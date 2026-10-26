//! Azure DevOps bearer tokens per tenant. Tokens live in memory only; Azure
//! CLI owns the refresh token and its own cache.

use super::az_cli::{self, AzVersion};
use super::tenant::TenantId;
use crate::cli_process::{CommandRunner, Invocation, Limits, SystemRunner};
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

/// Refresh this long before `expires_on` so requests never carry a dying token.
pub const REFRESH_MARGIN_SECONDS: i64 = 5 * 60;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub struct AccessToken {
    secret: Zeroizing<String>,
    pub expires_on: i64,
}

impl AccessToken {
    pub(super) fn new(secret: Zeroizing<String>, expires_on: i64) -> Self {
        Self { secret, expires_on }
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessToken")
            .field("secret", &"[REDACTED]")
            .field("expires_on", &self.expires_on)
            .finish()
    }
}

/// Typed cause of a failed token request, classified once at the Azure CLI
/// process boundary. Carries no Azure CLI text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    AzMissing,
    /// Older than [`az_cli::MIN_VERSION`].
    AzTooOld,
    NotLoggedIn,
    TenantNotFound,
    NoAccountInTenant,
    /// The resource tenant's cross-tenant access policy blocks this user;
    /// signing in again cannot fix it.
    TenantAccessBlocked,
    /// MFA, Conditional Access, device compliance, consent or an expired
    /// sign-in session: needs a user-initiated sign-in.
    InteractionRequired {
        aadsts: u32,
    },
    Network,
    Timeout,
    MalformedOutput,
    Other {
        exit_code: Option<i32>,
        aadsts: Option<u32>,
    },
}

#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn token(&self, tenant: &TenantId) -> Result<AccessToken, TokenError>;
    /// Drops `rejected` from the cache after the provider refused it (401).
    /// A newer cached token is kept, so concurrent 401s cause one refresh.
    fn invalidate(&self, tenant: &TenantId, rejected: &AccessToken);
}

/// Outcome of one provider call made with a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection<E> {
    /// The provider answered 401.
    Unauthorized,
    Failed(E),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedError<E> {
    Token(TokenError),
    /// Still 401 after one invalidate and retry.
    Unauthorized,
    Failed(E),
}

/// Runs `call` with a token; on 401 invalidates it and retries exactly once.
pub async fn with_token<S, T, E, F, Fut>(
    source: &S,
    tenant: &TenantId,
    mut call: F,
) -> Result<T, AuthorizedError<E>>
where
    S: TokenSource + ?Sized,
    F: FnMut(AccessToken) -> Fut,
    Fut: Future<Output = Result<T, Rejection<E>>>,
{
    let mut retried = false;
    loop {
        let token = source.token(tenant).await.map_err(AuthorizedError::Token)?;
        match call(token.clone()).await {
            Ok(value) => return Ok(value),
            Err(Rejection::Failed(error)) => return Err(AuthorizedError::Failed(error)),
            Err(Rejection::Unauthorized) if retried => return Err(AuthorizedError::Unauthorized),
            Err(Rejection::Unauthorized) => {
                source.invalidate(tenant, &token);
                retried = true;
            }
        }
    }
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default()
    })
}

#[derive(Default)]
struct Slot {
    cached: Mutex<Option<AccessToken>>,
    /// Held across the Azure CLI run: one run in flight per tenant.
    flight: tokio::sync::Mutex<()>,
}

pub struct AzCliTokenSource<R> {
    executable: Option<PathBuf>,
    runner: R,
    clock: Clock,
    timeout: Duration,
    supported: tokio::sync::OnceCell<AzVersion>,
    slots: Mutex<HashMap<TenantId, Arc<Slot>>>,
}

impl AzCliTokenSource<SystemRunner> {
    pub fn discover() -> Self {
        Self::new(az_cli::discover(), SystemRunner, system_clock())
    }
}

impl<R: CommandRunner> AzCliTokenSource<R> {
    pub fn new(executable: Option<PathBuf>, runner: R, clock: Clock) -> Self {
        Self {
            executable,
            runner,
            clock,
            timeout: DEFAULT_TIMEOUT,
            supported: tokio::sync::OnceCell::new(),
            slots: Mutex::default(),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs `az version`; uncached.
    pub async fn installed_version(&self) -> Result<AzVersion, TokenError> {
        let stdout = self.run(&["version", "--output", "json"]).await?;
        az_cli::parse_version(&stdout)
    }

    async fn ensure_supported(&self) -> Result<(), TokenError> {
        // Only a supported version is remembered, so an upgrade is seen on retry.
        self.supported
            .get_or_try_init(|| async {
                let version = self.installed_version().await?;
                if version < az_cli::MIN_VERSION {
                    return Err(TokenError::AzTooOld);
                }
                Ok(version)
            })
            .await
            .map(|_| ())
    }

    async fn run(&self, args: &[&str]) -> Result<Zeroizing<Vec<u8>>, TokenError> {
        let program = self.executable.as_deref().ok_or(TokenError::AzMissing)?;
        let output = self
            .runner
            .run(Invocation {
                program,
                args,
                env: &az_cli::ENV,
                limits: Limits {
                    timeout: self.timeout,
                    output_bytes: OUTPUT_LIMIT,
                },
            })
            .await
            .map_err(az_cli::run_error)?;
        if output.code != Some(0) {
            return Err(az_cli::classify(output.code, &output.stderr));
        }
        Ok(output.stdout)
    }

    fn slot(&self, tenant: &TenantId) -> Arc<Slot> {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(tenant.clone())
            .or_default()
            .clone()
    }

    fn fresh(&self, slot: &Slot) -> Option<AccessToken> {
        let cached = slot
            .cached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cached
            .as_ref()
            .filter(|token| (self.clock)() < token.expires_on - REFRESH_MARGIN_SECONDS)
            .cloned()
    }
}

#[async_trait]
impl<R: CommandRunner> TokenSource for AzCliTokenSource<R> {
    async fn token(&self, tenant: &TenantId) -> Result<AccessToken, TokenError> {
        let slot = self.slot(tenant);
        if let Some(token) = self.fresh(&slot) {
            return Ok(token);
        }
        let _flight = slot.flight.lock().await;
        if let Some(token) = self.fresh(&slot) {
            return Ok(token);
        }
        self.ensure_supported().await?;
        let stdout = self
            .run(&[
                "account",
                "get-access-token",
                "--resource",
                super::RESOURCE,
                "--tenant",
                tenant.as_str(),
                "--output",
                "json",
            ])
            .await?;
        let token = az_cli::parse_token(&stdout)?;
        *slot
            .cached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(token.clone());
        Ok(token)
    }

    fn invalidate(&self, tenant: &TenantId, rejected: &AccessToken) {
        let slot = self.slot(tenant);
        let mut cached = slot
            .cached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cached
            .as_ref()
            .is_some_and(|token| token.secret() == rejected.secret())
        {
            *cached = None;
        }
    }
}

#[cfg(test)]
mod tests;
