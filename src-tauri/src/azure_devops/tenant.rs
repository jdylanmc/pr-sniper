//! Organization → Microsoft Entra tenant discovery. Azure DevOps answers an
//! unauthenticated `HEAD https://dev.azure.com/{organization}` with the
//! organization's tenant in `X-VSS-ResourceTenant`; unknown organizations 404.

use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantId(String);

impl TenantId {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let groups: Vec<_> = value.split('-').collect();
        let shaped = groups.len() == 5
            && groups
                .iter()
                .zip([8, 4, 4, 4, 12])
                .all(|(group, len)| group.len() == len)
            && groups
                .iter()
                .all(|group| group.bytes().all(|b| b.is_ascii_hexdigit()));
        shaped.then(|| Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_zero(&self) -> bool {
        self.0.bytes().all(|b| b == b'0' || b == b'-')
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantDiscoveryError {
    InvalidOrganization,
    /// 404: no such organization.
    OrganizationNotFound,
    /// No usable `X-VSS-ResourceTenant` header.
    MissingTenant,
    /// All-zero tenant (Microsoft account-backed organization); Azure CLI
    /// cannot target it.
    ZeroTenant,
    /// 429 or 5xx.
    Unavailable,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadResponse {
    pub status: u16,
    pub resource_tenant: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkError;

#[async_trait]
pub trait HeadTransport: Send + Sync {
    /// Unauthenticated HEAD without following redirects.
    async fn head(&self, url: &str) -> Result<HeadResponse, NetworkError>;
}

pub struct HttpHeadTransport(reqwest::Client);

impl HttpHeadTransport {
    pub fn new() -> Result<Self, NetworkError> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map(Self)
            .map_err(|_| NetworkError)
    }
}

#[async_trait]
impl HeadTransport for HttpHeadTransport {
    async fn head(&self, url: &str) -> Result<HeadResponse, NetworkError> {
        let response = self.0.head(url).send().await.map_err(|_| NetworkError)?;
        Ok(HeadResponse {
            status: response.status().as_u16(),
            resource_tenant: response
                .headers()
                .get("x-vss-resourcetenant")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
    }
}

/// Azure DevOps organization names: letters, digits and inner hyphens.
pub fn valid_organization(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 50
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn resource_tenant(response: &HeadResponse) -> Result<TenantId, TenantDiscoveryError> {
    match response.status {
        404 => return Err(TenantDiscoveryError::OrganizationNotFound),
        429 | 500..=599 => return Err(TenantDiscoveryError::Unavailable),
        _ => {}
    }
    let tenant = response
        .resource_tenant
        .as_deref()
        .and_then(TenantId::parse)
        .ok_or(TenantDiscoveryError::MissingTenant)?;
    if tenant.is_zero() {
        return Err(TenantDiscoveryError::ZeroTenant);
    }
    Ok(tenant)
}

pub async fn discover(
    transport: &(impl HeadTransport + ?Sized),
    organization: &str,
) -> Result<TenantId, TenantDiscoveryError> {
    if !valid_organization(organization) {
        return Err(TenantDiscoveryError::InvalidOrganization);
    }
    let response = transport
        .head(&format!("https://dev.azure.com/{organization}"))
        .await
        .map_err(|_| TenantDiscoveryError::Network)?;
    resource_tenant(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeTransport {
        response: Result<HeadResponse, NetworkError>,
        urls: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        fn new(status: u16, tenant: Option<&str>) -> Self {
            Self {
                response: Ok(HeadResponse {
                    status,
                    resource_tenant: tenant.map(str::to_owned),
                }),
                urls: Mutex::default(),
            }
        }
    }

    #[async_trait]
    impl HeadTransport for FakeTransport {
        async fn head(&self, url: &str) -> Result<HeadResponse, NetworkError> {
            self.urls.lock().unwrap().push(url.into());
            self.response.clone()
        }
    }

    #[tokio::test]
    async fn finds_the_organization_tenant_from_the_unauthenticated_challenge() {
        let transport = FakeTransport::new(401, Some("0000000A-0000-0000-0000-000000000001"));
        let tenant = discover(&transport, "contoso").await.unwrap();
        assert_eq!(tenant.as_str(), "0000000a-0000-0000-0000-000000000001");
        assert_eq!(
            *transport.urls.lock().unwrap(),
            ["https://dev.azure.com/contoso"]
        );
    }

    #[tokio::test]
    async fn distinguishes_unknown_organization_missing_and_zero_tenant() {
        let cases = [
            (404, None, TenantDiscoveryError::OrganizationNotFound),
            (401, None, TenantDiscoveryError::MissingTenant),
            (401, Some("not-a-guid"), TenantDiscoveryError::MissingTenant),
            (
                401,
                Some("00000000-0000-0000-0000-000000000000"),
                TenantDiscoveryError::ZeroTenant,
            ),
            (503, None, TenantDiscoveryError::Unavailable),
            (429, None, TenantDiscoveryError::Unavailable),
        ];
        for (status, tenant, expected) in cases {
            let transport = FakeTransport::new(status, tenant);
            assert_eq!(
                discover(&transport, "contoso").await,
                Err(expected),
                "{status}"
            );
        }
        let offline = FakeTransport {
            response: Err(NetworkError),
            urls: Mutex::default(),
        };
        assert_eq!(
            discover(&offline, "contoso").await,
            Err(TenantDiscoveryError::Network)
        );
    }

    #[tokio::test]
    async fn rejects_organization_names_that_could_alter_the_url_without_a_request() {
        assert!(valid_organization("contoso-dev"));
        let transport = FakeTransport::new(401, Some("00000000-0000-0000-0000-000000000001"));
        for name in [
            "",
            "-a",
            "a-",
            "a/b",
            "a?b",
            "a.b",
            "a b",
            "a@b",
            &"a".repeat(51),
        ] {
            assert_eq!(
                discover(&transport, name).await,
                Err(TenantDiscoveryError::InvalidOrganization),
                "{name}"
            );
        }
        assert!(transport.urls.lock().unwrap().is_empty());
    }
}
