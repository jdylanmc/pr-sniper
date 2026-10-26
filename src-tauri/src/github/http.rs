use super::{
    credentials::Credential,
    oauth::TokenPair,
    provider::{Response, Transport},
    ConnectionError,
};
use reqwest::{
    blocking::{Client, Request},
    header::{HeaderValue, ACCEPT, AUTHORIZATION},
    redirect::Policy,
    Url,
};
use std::{collections::BTreeMap, io::Read, time::Duration};

pub struct HttpTransport {
    client: Client,
    /// Credential-free client for signed redirect targets; never sees `credential`.
    blob: Client,
    credential: Credential,
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
            credential,
        })
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

    fn request(&self, path: &str) -> Result<Request, ConnectionError> {
        if !path.starts_with('/') || path.starts_with("//") || path.contains('#') {
            return Err(ConnectionError::InvalidResponse);
        }
        let url = Url::parse(&format!("https://api.github.com{path}"))
            .map_err(|_| ConnectionError::InvalidResponse)?;
        if url.host_str() != Some("api.github.com") || url.scheme() != "https" {
            return Err(ConnectionError::InvalidResponse);
        }
        let value = zeroize::Zeroizing::new(format!("Bearer {}", *self.credential.0));
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
        self.execute(self.request(path)?)
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
        let mut request = self.request(path)?;
        let (method, body) = match mutation {
            super::publication::Request::Post(body) => (reqwest::Method::POST, Some(body)),
            super::publication::Request::Delete => (reqwest::Method::DELETE, None),
        };
        *request.method_mut() = method;
        if let Some(body) = body {
            request.headers_mut().insert(
                reqwest::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            *request.body_mut() = Some(
                serde_json::to_vec(&body)
                    .map_err(|_| ConnectionError::InvalidResponse)?
                    .into(),
            );
        }

        self.execute(request)
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
        let request = transport.request("/user").unwrap();
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
    fn credential_cannot_be_forwarded_to_an_external_url() {
        let transport =
            HttpTransport::new(Credential(Zeroizing::new("fixture-secret".into()))).unwrap();
        for path in [
            "//evil.example/user",
            "https://evil.example/user",
            "/user#secret",
        ] {
            assert_eq!(
                transport.request(path).unwrap_err(),
                ConnectionError::InvalidResponse
            );
        }
    }
}
