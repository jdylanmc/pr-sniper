use super::{
    credentials::Credential,
    oauth::TokenPair,
    provider::{Response, Transport},
    publication::SendFailure,
    ConnectionError,
};
use reqwest::{
    blocking::{Client, Request},
    header::{HeaderValue, ACCEPT, AUTHORIZATION},
    redirect::Policy,
    Url,
};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Arc, RwLock},
    time::Duration,
};

/// Replaces an access token GitHub refused with a refreshed one. Errors keep
/// their typed cause: only a refused refresh is `SignedOut`.
pub type Renewal = Box<dyn Fn(&str) -> Result<TokenPair, ConnectionError> + Send + Sync>;

pub struct HttpTransport {
    client: Client,
    /// Credential-free client for signed redirect targets; never sees `credential`.
    blob: Client,
    credential: RwLock<Arc<Credential>>,
    renewal: Option<Renewal>,
}

/// Hard stop for a redirected blob download; the tail kept is far smaller.
const MAX_BLOB_STREAM_BYTES: usize = 256 * 1024 * 1024;

impl HttpTransport {
    pub fn new(credential: Credential) -> Result<Self, ConnectionError> {
        let client = Client::builder()
            .user_agent("PR-Sniper/0.1")
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| ConnectionError::Network)?;
        let blob = Client::builder()
            .user_agent("PR-Sniper/0.1")
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| ConnectionError::Network)?;
        Ok(Self {
            client,
            blob,
            credential: RwLock::new(Arc::new(credential)),
            renewal: None,
        })
    }

    /// A token-pair transport that refreshes once and retries when GitHub
    /// answers 401, so a refused but renewable token never reads as expired.
    pub fn from_token_pair_renewing(
        pair: &TokenPair,
        renewal: Renewal,
    ) -> Result<Self, ConnectionError> {
        let mut transport = Self::from_token_pair(pair)?;
        transport.renewal = Some(renewal);
        Ok(transport)
    }

    fn current_credential(&self) -> Result<Arc<Credential>, ConnectionError> {
        self.credential
            .read()
            .map(|credential| credential.clone())
            .map_err(|_| ConnectionError::Configuration)
    }

    /// Sends with the current credential; on 401 renews it once and resends.
    fn authorized(
        &self,
        build: impl Fn(&Credential) -> Result<Request, ConnectionError>,
    ) -> Result<Response, ConnectionError> {
        self.authorized_send(build).map_err(|failure| failure.error)
    }

    fn authorized_send(
        &self,
        build: impl Fn(&Credential) -> Result<Request, ConnectionError>,
    ) -> Result<Response, SendFailure> {
        let renewal = self.renewal.as_ref().map(|renew| {
            move |rejected: &Arc<Credential>| -> Result<Arc<Credential>, ConnectionError> {
                let pair = renew(rejected.0.as_str())?;
                let fresh = Arc::new(Credential::from_secret(pair.access_token()));
                *self
                    .credential
                    .write()
                    .map_err(|_| ConnectionError::Configuration)? = fresh.clone();
                Ok(fresh)
            }
        });
        let credential = self.current_credential().map_err(|error| SendFailure {
            error,
            uncertain: false,
        })?;
        with_renewal(
            credential,
            |credential| self.execute(build(credential)?),
            renewal.as_ref(),
        )
    }

    /// A redirect target request: absolute `https`, no user info, no credential header.
    fn blob_request(&self, url: &str) -> Result<Request, ConnectionError> {
        let url = Url::parse(url).map_err(|_| ConnectionError::InvalidResponse)?;
        if url.scheme() != "https"
            || url.host_str().is_none_or(str::is_empty)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ConnectionError::InvalidResponse);
        }
        self.blob
            .get(url)
            .build()
            .map_err(|_| ConnectionError::Network)
    }

    pub fn from_token_pair(pair: &TokenPair) -> Result<Self, ConnectionError> {
        Self::new(Credential::from_secret(pair.access_token()))
    }

    fn request(&self, path: &str, credential: &Credential) -> Result<Request, ConnectionError> {
        if !path.starts_with('/') || path.starts_with("//") || path.contains('#') {
            return Err(ConnectionError::InvalidResponse);
        }
        let url = Url::parse(&format!("https://api.github.com{path}"))
            .map_err(|_| ConnectionError::InvalidResponse)?;
        if url.host_str() != Some("api.github.com") || url.scheme() != "https" {
            return Err(ConnectionError::InvalidResponse);
        }
        let value = zeroize::Zeroizing::new(format!("Bearer {}", *credential.0));
        let mut authorization =
            HeaderValue::from_str(&value).map_err(|_| ConnectionError::BrokenCli)?;
        authorization.set_sensitive(true);
        self.client
            .get(url)
            .header(AUTHORIZATION, authorization)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .build()
            .map_err(|_| ConnectionError::Network)
    }
}

impl Transport for HttpTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.authorized(|credential| self.request(path, credential))
    }

    fn get_unauthenticated_tail(
        &self,
        url: &str,
        tail: usize,
    ) -> Result<(Response, bool), ConnectionError> {
        let request = self.blob_request(url)?;
        let mut response = self.blob.execute(request).map_err(send_error)?;
        let status = response.status().as_u16();
        let headers = captured(response.headers())?;
        let mut body = Vec::new();
        let mut dropped = false;
        let mut seen = 0_usize;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|_| ConnectionError::Network)?;
            if read == 0 {
                break;
            }
            seen += read;
            if seen > MAX_BLOB_STREAM_BYTES {
                return Err(ConnectionError::LimitExceeded);
            }
            body.extend_from_slice(&buffer[..read]);
            if body.len() > tail.saturating_mul(2) {
                body.drain(..body.len() - tail);
                dropped = true;
            }
        }
        if body.len() > tail {
            body.drain(..body.len() - tail);
            dropped = true;
        }
        Ok((
            Response {
                status,
                headers,
                body,
            },
            dropped,
        ))
    }
}

fn send_error(error: reqwest::Error) -> ConnectionError {
    if error.is_timeout() {
        ConnectionError::Timeout
    } else {
        ConnectionError::Network
    }
}

fn captured(
    source: &reqwest::header::HeaderMap,
) -> Result<BTreeMap<String, String>, ConnectionError> {
    let mut headers = BTreeMap::new();
    for name in [
        "link",
        "location",
        "x-oauth-scopes",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
        "retry-after",
        "x-github-sso",
    ] {
        if let Some(value) = source.get(name) {
            headers.insert(
                name.to_string(),
                value
                    .to_str()
                    .map_err(|_| ConnectionError::InvalidResponse)?
                    .to_string(),
            );
        }
    }
    Ok(headers)
}

impl super::publication::MutationTransport for HttpTransport {
    fn mutate(
        &self,
        path: &str,
        mutation: super::publication::Request,
    ) -> Result<Response, ConnectionError> {
        self.send_mutation(path, mutation)
            .map_err(|failure| failure.error)
    }

    fn send_mutation(
        &self,
        path: &str,
        mutation: super::publication::Request,
    ) -> Result<Response, SendFailure> {
        let (method, body) = match mutation {
            super::publication::Request::Post(body) => (
                reqwest::Method::POST,
                Some(serde_json::to_vec(&body).map_err(|_| SendFailure {
                    error: ConnectionError::InvalidResponse,
                    uncertain: false,
                })?),
            ),
            super::publication::Request::Delete => (reqwest::Method::DELETE, None),
        };
        // A 401 means GitHub did not apply the mutation, so a resend is safe.
        self.authorized_send(|credential| {
            let mut request = self.request(path, credential)?;
            *request.method_mut() = method.clone();
            if let Some(body) = &body {
                request.headers_mut().insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                *request.body_mut() = Some(body.clone().into());
            }
            Ok(request)
        })
    }
}

impl super::threads::QueryTransport for HttpTransport {
    fn query(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<Response, ConnectionError> {
        super::publication::MutationTransport::mutate(
            self,
            "/graphql",
            super::publication::Request::Post(
                serde_json::json!({"query":query,"variables":variables}),
            ),
        )
    }
}

impl HttpTransport {
    fn execute(&self, request: Request) -> Result<Response, ConnectionError> {
        let response = self.client.execute(request).map_err(send_error)?;
        let status = response.status().as_u16();
        let headers = captured(response.headers())?;
        let mut body = Vec::new();
        response
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut body)
            .map_err(|_| ConnectionError::Network)?;
        if body.len() > 32 * 1024 * 1024 {
            return Err(ConnectionError::LimitExceeded);
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

/// Sends with `credential`; a 401 renews it once and resends. The second
/// answer stands, so a 401 on a just-refreshed token stays `SignedOut`.
/// A failed renewal is certain: GitHub refused the first send and no resend
/// left, so nothing was applied.
fn with_renewal<C, R: Fn(&C) -> Result<C, ConnectionError>>(
    credential: C,
    send: impl Fn(&C) -> Result<Response, ConnectionError>,
    renew: Option<R>,
) -> Result<Response, SendFailure> {
    let sent = |credential: &C| {
        send(credential).map_err(|error| SendFailure {
            error,
            uncertain: true,
        })
    };
    let response = sent(&credential)?;
    match renew {
        Some(renew) if response.status == 401 => {
            sent(&renew(&credential).map_err(|error| SendFailure {
                error,
                uncertain: false,
            })?)
        }
        _ => Ok(response),
    }
}

pub async fn current_identity_async(pair: &TokenPair) -> Result<super::Identity, ConnectionError> {
    let client = reqwest::Client::builder()
        .user_agent("PR-Sniper/0.1")
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| ConnectionError::Network)?;
    let secret = zeroize::Zeroizing::new(format!("Bearer {}", pair.access_token()));
    let mut authorization =
        HeaderValue::from_str(&secret).map_err(|_| ConnectionError::InvalidResponse)?;
    authorization.set_sensitive(true);
    let mut response = client
        .get("https://api.github.com/user")
        .header(AUTHORIZATION, authorization)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                ConnectionError::Timeout
            } else {
                ConnectionError::Network
            }
        })?;
    let status = response.status().as_u16();
    let mut headers = BTreeMap::new();
    for name in [
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
        "retry-after",
        "x-github-sso",
    ] {
        if let Some(value) = response.headers().get(name) {
            headers.insert(
                name.into(),
                value
                    .to_str()
                    .map_err(|_| ConnectionError::InvalidResponse)?
                    .into(),
            );
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ConnectionError::Network)?
    {
        if body.len() + chunk.len() > 64 * 1024 {
            return Err(ConnectionError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    let (value, _) = super::provider::parse_response(Response {
        status,
        headers,
        body,
    })?;
    super::verify_identity(&value, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    #[test]
    fn authenticated_request_is_get_only_and_debug_redacts_secret() {
        let transport =
            HttpTransport::new(Credential(Zeroizing::new("fixture-secret".into()))).unwrap();
        let request = transport
            .request("/user", &transport.current_credential().unwrap())
            .unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().as_str(), "https://api.github.com/user");
        assert!(request.headers()[AUTHORIZATION].is_sensitive());
        assert!(!format!("{request:?}").contains("fixture-secret"));
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer fixture-secret");
        assert!(request.body().is_none());
    }

    #[test]
    fn redirect_target_request_never_carries_the_credential() {
        let transport =
            HttpTransport::new(Credential(Zeroizing::new("fixture-secret".into()))).unwrap();
        let request = transport
            .blob_request("https://results.example.blob.core.windows.net/log?sig=x")
            .unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert!(request.headers().get(AUTHORIZATION).is_none());
        assert!(!format!("{request:?}").contains("fixture-secret"));
        for url in [
            "http://results.example/log",
            "https://user:pw@results.example/log",
            "/repos/a/b/actions/jobs/1/logs",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                transport.blob_request(url).unwrap_err(),
                ConnectionError::InvalidResponse
            );
        }
    }

    #[test]
    fn transport_failures_are_network_or_timeout_never_permanent() {
        use std::io::Write;
        use std::net::TcpListener;
        let transport = HttpTransport {
            client: Client::builder()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
            blob: Client::new(),
            credential: RwLock::new(Arc::new(Credential(Zeroizing::new(
                "fixture-secret".into(),
            )))),
            renewal: None,
        };
        let get = |url: &str| transport.execute(transport.client.get(url).build().unwrap());
        let fail = |url: &str| get(url).err().expect("a transport failure");
        // Serves one connection with `reply`, then closes it.
        let serve = |reply: &'static [u8], hold: Duration| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request);
                let _ = socket.write_all(reply);
                std::thread::sleep(hold);
            });
            (address, server)
        };
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let refused = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        // Windows retries a refused connect until the client timeout; a slow
        // resolver may time out on `.invalid` too. Either way: transport class.
        for url in [refused.as_str(), "http://pr-sniper-fixture.invalid/"] {
            assert!(
                matches!(
                    fail(url),
                    ConnectionError::Network | ConnectionError::Timeout
                ),
                "{url}"
            );
        }
        for (name, reply, hold, error) in [
            (
                "truncated body",
                &b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"id\":"[..],
                Duration::ZERO,
                ConnectionError::Network,
            ),
            (
                "truncated chunk",
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n{\"id\":",
                Duration::ZERO,
                ConnectionError::Network,
            ),
            (
                "reset before headers",
                b"",
                Duration::ZERO,
                ConnectionError::Network,
            ),
            (
                "read timeout",
                b"",
                Duration::from_secs(3),
                ConnectionError::Timeout,
            ),
        ] {
            let (address, server) = serve(reply, hold);
            assert_eq!(fail(&format!("http://{address}/")), error, "{name}");
            server.join().unwrap();
        }
        // TLS handshake against a plain-text peer.
        let (address, server) = serve(b"HTTP/1.1 200 OK\r\n\r\n", Duration::ZERO);
        assert_eq!(
            fail(&format!("https://{address}/")),
            ConnectionError::Network
        );
        server.join().unwrap();
        // A 5xx is a response: a transient provider failure, not a transport one.
        let (address, server) = serve(
            b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let Ok(response) = get(&format!("http://{address}/")) else {
            panic!("a 5xx is a response");
        };
        assert_eq!(
            super::super::provider::parse_response(response).err(),
            Some(ConnectionError::ProviderFailure)
        );
        server.join().unwrap();
    }

    #[test]
    fn credential_cannot_be_forwarded_to_an_external_url() {
        let transport =
            HttpTransport::new(Credential(Zeroizing::new("fixture-secret".into()))).unwrap();
        for path in [
            "//evil.example/user",
            "https://evil.example/user",
            "/user#secret",
        ] {
            assert_eq!(
                transport
                    .request(path, &transport.current_credential().unwrap())
                    .unwrap_err(),
                ConnectionError::InvalidResponse
            );
        }
    }

    fn status(status: u16) -> Response {
        Response {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn a_refused_token_is_renewed_once_and_resent() {
        let sent = std::cell::RefCell::new(Vec::new());
        let renewals = std::cell::Cell::new(0);
        let send = |token: &&'static str| {
            sent.borrow_mut().push(*token);
            Ok(status(if *token == "stale" { 401 } else { 200 }))
        };
        let renew = |_: &&str| {
            renewals.set(renewals.get() + 1);
            Ok("fresh")
        };
        let response = with_renewal("stale", send, Some(&renew)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(*sent.borrow(), ["stale", "fresh"]);
        assert_eq!(renewals.get(), 1);
    }

    #[test]
    fn a_renewal_that_cannot_reach_github_keeps_its_transient_cause() {
        for cause in [
            ConnectionError::Network,
            ConnectionError::Timeout,
            ConnectionError::ProviderFailure,
        ] {
            let renew = |_: &&str| Err(cause);
            // GitHub refused the send and nothing was resent: certain.
            assert_eq!(
                with_renewal("stale", |_| Ok(status(401)), Some(&renew)).err(),
                Some(SendFailure {
                    error: cause,
                    uncertain: false
                })
            );
        }
    }

    #[test]
    fn a_refused_refresh_or_a_second_refusal_is_definitive() {
        let refused = |_: &&str| Err(ConnectionError::SignedOut);
        assert_eq!(
            with_renewal("stale", |_| Ok(status(401)), Some(&refused)).err(),
            Some(SendFailure {
                error: ConnectionError::SignedOut,
                uncertain: false
            })
        );
        let renew = |_: &&str| Ok("fresh");
        let response = with_renewal("stale", |_| Ok(status(401)), Some(&renew)).unwrap();
        assert_eq!(response.status, 401);
    }

    #[test]
    fn only_a_401_renews() {
        let renew = |_: &&str| -> Result<&str, ConnectionError> { panic!("no renewal") };
        for code in [200, 403, 404, 429, 500, 503] {
            let response = with_renewal("token", |_| Ok(status(code)), Some(&renew)).unwrap();
            assert_eq!(response.status, code);
        }
        // A send that may have reached GitHub stays uncertain.
        assert_eq!(
            with_renewal("token", |_| Err(ConnectionError::Network), Some(&renew)).err(),
            Some(SendFailure {
                error: ConnectionError::Network,
                uncertain: true
            })
        );
    }
}
