use super::{
    credentials::Credential,
    device_flow::TokenPair,
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
    credential: Credential,
}

impl HttpTransport {
    pub fn new(credential: Credential) -> Result<Self, ConnectionError> {
        let client = Client::builder()
            .user_agent("PR-Sniper/0.1")
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| ConnectionError::Network)?;
        Ok(Self { client, credential })
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
        let response = self.client.execute(self.request(path)?).map_err(|error| {
            if error.is_timeout() {
                ConnectionError::Timeout
            } else {
                ConnectionError::Network
            }
        })?;
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for name in [
            "link",
            "x-oauth-scopes",
            "x-ratelimit-remaining",
            "retry-after",
        ] {
            if let Some(value) = response.headers().get(name) {
                headers.insert(
                    name.to_string(),
                    value
                        .to_str()
                        .map_err(|_| ConnectionError::InvalidResponse)?
                        .to_string(),
                );
            }
        }
        let mut body = Vec::new();
        response
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut body)
            .map_err(|_| ConnectionError::Network)?;
        if body.len() > 32 * 1024 * 1024 {
            return Err(ConnectionError::IncompleteRead);
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
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
