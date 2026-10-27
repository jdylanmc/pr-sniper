//! "Connect Azure DevOps" setup (FR-20–24): Azure CLI, organization and
//! account state, plus the user-initiated install and sign-in actions.
//!
//! Probing is never interactive. `az login` and package-manager installs
//! run only from [`Setup::sign_in`] and [`Setup::install`], which only the
//! Settings commands call. PR Sniper never runs `az logout`.

use super::az_cli::{self, AzVersion};
use super::connection::{self, ConnectionError, ConnectionTransport, Identity};
use super::tenant::{self, HeadTransport, TenantDiscoveryError, TenantId};
use super::token_source::{AuthorizedError, AzCliTokenSource, Clock, TokenError, TokenSource};
use crate::cli_process::{CommandRunner, EnvPolicy, Invocation, Limits};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const INSTALL_PAGE: &str = "https://github.com/Azure/azure-cli#installation";
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const WINGET_ID: &str = "Microsoft.AzureCLI";

const HOMEBREW_ENV: EnvPolicy = EnvPolicy {
    remove: &[],
    set: &[("NONINTERACTIVE", "1"), ("HOMEBREW_NO_ENV_HINTS", "1")],
};
const WINGET_ENV: EnvPolicy = EnvPolicy {
    remove: &[],
    set: &[],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Other,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Az,
    Homebrew,
    Winget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Installer {
    Homebrew,
    Winget,
    /// Opens [`INSTALL_PAGE`].
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallPlan {
    Run {
        installer: Installer,
        program: PathBuf,
        args: Vec<&'static str>,
    },
    Page,
}

impl InstallPlan {
    pub fn installer(&self) -> Installer {
        match self {
            Self::Run { installer, .. } => *installer,
            Self::Page => Installer::Page,
        }
    }
}

/// Package-manager command for installing (or, when `upgrade`, updating)
/// Azure CLI; dependencies come with the package. Otherwise the install page.
pub fn install_plan(
    platform: Platform,
    upgrade: bool,
    homebrew: Option<PathBuf>,
    winget: Option<PathBuf>,
) -> InstallPlan {
    match (platform, homebrew, winget) {
        (Platform::MacOs, Some(program), _) => InstallPlan::Run {
            installer: Installer::Homebrew,
            program,
            args: vec![if upgrade { "upgrade" } else { "install" }, "azure-cli"],
        },
        (Platform::Windows, _, Some(program)) => InstallPlan::Run {
            installer: Installer::Winget,
            program,
            // stdin is closed: accept the agreements the user's click implies
            // and stay on the winget source so msstore terms never prompt.
            args: vec![
                if upgrade { "upgrade" } else { "install" },
                "-e",
                "--id",
                WINGET_ID,
                "--source",
                "winget",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ],
        },
        _ => InstallPlan::Page,
    }
}

/// `az login` arguments for one tenant, scoped to Azure DevOps.
pub fn sign_in_args(tenant: &TenantId) -> Vec<String> {
    vec![
        "login".into(),
        "--tenant".into(),
        tenant.as_str().into(),
        "--allow-no-subscriptions".into(),
        "--scope".into(),
        format!("{}/.default", super::RESOURCE),
        "-o".into(),
        "none".into(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CliView {
    Unknown,
    Missing,
    TooOld {
        version: String,
    },
    /// Found, but `az version` failed (timeout, exit, malformed output).
    /// Not installable; Re-check only.
    Failed,
    Installing {
        version: Option<String>,
    },
    Ready {
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OrganizationView {
    Unset,
    Unknown {
        name: String,
    },
    NotFound {
        name: String,
    },
    /// No usable Microsoft Entra tenant (missing or all-zero).
    TenantUnknown {
        name: String,
    },
    Offline {
        name: String,
    },
    TenantKnown {
        name: String,
        tenant: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccountView {
    /// Azure CLI or the tenant is not ready yet.
    Unavailable,
    SignedOut,
    SigningIn,
    InteractionRequired,
    TenantAccessBlocked,
    Offline,
    Failed,
    Connected {
        identity: Identity,
    },
    WrongIdentity {
        identity: Identity,
        expected: Identity,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupView {
    pub cli: CliView,
    pub minimum: String,
    pub installer: Installer,
    pub organization: OrganizationView,
    pub account: AccountView,
    pub checking: bool,
}

/// Saved non-secret binding: the organization and the Azure DevOps identity
/// that owns it in PR Sniper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupError {
    Busy,
    NotReady,
    InvalidOrganization,
    Storage,
    BrowserOpen,
}

impl From<SetupError> for String {
    fn from(error: SetupError) -> Self {
        serde_json::to_value(error)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

type Locate = dyn Fn(Program) -> Option<PathBuf> + Send + Sync;
type OpenPage = dyn Fn(&str) -> bool + Send + Sync;
type Persist = dyn Fn(&Binding) -> Result<(), ()> + Send + Sync;

pub struct Backends {
    pub runner: Arc<dyn CommandRunner>,
    pub head: Arc<dyn HeadTransport>,
    pub connection: Arc<dyn ConnectionTransport>,
    pub clock: Clock,
    pub platform: Platform,
    /// Absolute path of a program, never through a shell.
    pub locate: Arc<Locate>,
    pub open_page: Arc<OpenPage>,
    pub persist: Arc<Persist>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub cli: CliView,
    pub installer: Installer,
    pub organization: OrganizationView,
    pub tenant: Option<TenantId>,
    pub account: AccountView,
    /// Identity to save as the binding.
    pub bind: Option<Identity>,
}

fn organization_view(
    name: &str,
    result: &Result<TenantId, TenantDiscoveryError>,
) -> OrganizationView {
    let name = name.to_owned();
    match result {
        Ok(tenant) => OrganizationView::TenantKnown {
            name,
            tenant: tenant.as_str().to_owned(),
        },
        Err(
            TenantDiscoveryError::OrganizationNotFound | TenantDiscoveryError::InvalidOrganization,
        ) => OrganizationView::NotFound { name },
        Err(TenantDiscoveryError::MissingTenant | TenantDiscoveryError::ZeroTenant) => {
            OrganizationView::TenantUnknown { name }
        }
        Err(TenantDiscoveryError::Network | TenantDiscoveryError::Unavailable) => {
            OrganizationView::Offline { name }
        }
    }
}

pub fn account_from_token_error(error: TokenError) -> AccountView {
    match error {
        TokenError::NotLoggedIn | TokenError::NoAccountInTenant | TokenError::TenantNotFound => {
            AccountView::SignedOut
        }
        TokenError::InteractionRequired { .. } => AccountView::InteractionRequired,
        TokenError::TenantAccessBlocked => AccountView::TenantAccessBlocked,
        TokenError::Network | TokenError::Timeout => AccountView::Offline,
        TokenError::AzMissing
        | TokenError::AzTooOld
        | TokenError::MalformedOutput
        | TokenError::Other { .. } => AccountView::Failed,
    }
}

/// One non-interactive pass over the setup state machine:
/// Azure CLI → organization → account.
pub async fn probe(backends: &Backends, binding: &Binding) -> Probed {
    let az = (backends.locate)(Program::Az);
    let installed = az.is_some();
    let source = AzCliTokenSource::new(
        az,
        Arc::clone(&backends.runner),
        Arc::clone(&backends.clock),
    );
    let version: Result<AzVersion, TokenError> = if installed {
        source.installed_version().await
    } else {
        Err(TokenError::AzMissing)
    };
    let cli = match version {
        Err(TokenError::AzMissing) => CliView::Missing,
        Err(_) => CliView::Failed,
        Ok(version) if version < az_cli::MIN_VERSION => CliView::TooOld {
            version: version.to_string(),
        },
        Ok(version) => CliView::Ready {
            version: version.to_string(),
        },
    };
    let installer = install_plan(
        backends.platform,
        false,
        (backends.locate)(Program::Homebrew),
        (backends.locate)(Program::Winget),
    )
    .installer();
    let (organization, tenant) = match binding.organization.as_deref() {
        None => (OrganizationView::Unset, None),
        Some(name) => {
            let result = tenant::discover(backends.head.as_ref(), name).await;
            (organization_view(name, &result), result.ok())
        }
    };
    let mut bind = None;
    let account = match (&cli, &tenant, binding.organization.as_deref()) {
        (CliView::Ready { .. }, Some(tenant), Some(name)) => {
            match connection::signed_in_identity(
                &source as &dyn TokenSource,
                tenant,
                backends.connection.as_ref(),
                name,
            )
            .await
            {
                Ok(identity) => match &binding.identity {
                    Some(expected) if expected.id != identity.id => AccountView::WrongIdentity {
                        identity,
                        expected: expected.clone(),
                    },
                    saved => {
                        if saved != &Some(identity.clone()) {
                            bind = Some(identity.clone());
                        }
                        AccountView::Connected { identity }
                    }
                },
                Err(AuthorizedError::Token(error)) => account_from_token_error(error),
                Err(AuthorizedError::Unauthorized) => AccountView::SignedOut,
                Err(AuthorizedError::Failed(ConnectionError::Network)) => AccountView::Offline,
                Err(AuthorizedError::Failed(_)) => AccountView::Failed,
            }
        }
        _ => AccountView::Unavailable,
    };
    Probed {
        cli,
        installer,
        organization,
        tenant,
        account,
        bind,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Install,
    SignIn,
}

struct Job {
    id: u64,
    kind: JobKind,
    task: tokio::task::JoinHandle<()>,
}

struct Inner {
    binding: Binding,
    probed: Option<Probed>,
    job: Option<Job>,
    next_job: u64,
    generation: u64,
    probing: usize,
}

/// Setup state shared by the Settings commands. Install and sign-in run as
/// one tracked, cancellable child at a time and re-probe when they end.
pub struct Setup {
    backends: Backends,
    inner: Mutex<Inner>,
    /// One token cache per located Azure CLI, shared by repository reads.
    tokens: Mutex<Option<(PathBuf, Arc<dyn TokenSource>)>>,
}

/// The bound identity as a repository account, as control tools list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryAccount {
    /// `connected` or `reconnect_required`.
    pub state: &'static str,
    pub provider: &'static str,
    pub account_id: String,
    pub login: String,
    pub organization: String,
    /// Azure DevOps reads are fenced by the bound identity, not a generation.
    pub connection_generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<crate::provider::ConnectionError>,
}

/// Setup state plus the bound repository account, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub setup: SetupView,
    pub account: Option<RepositoryAccount>,
}

/// A connected account: the organization, its tenant and the bound identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    pub organization: String,
    pub tenant: TenantId,
    pub identity: Identity,
}

impl Setup {
    pub fn new(backends: Backends, binding: Binding) -> Arc<Self> {
        Arc::new(Self {
            backends,
            inner: Mutex::new(Inner {
                binding,
                probed: None,
                job: None,
                next_job: 0,
                generation: 0,
                probing: 0,
            }),
            tokens: Mutex::default(),
        })
    }

    /// Drops cached read tokens: they belong to the previously bound identity.
    fn reset_tokens(&self) {
        *self
            .tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// The token source for reads; rebuilt when Azure CLI moves or the
    /// binding changes.
    pub fn token_source(&self) -> Arc<dyn TokenSource> {
        let az = (self.backends.locate)(Program::Az);
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let (Some((path, source)), Some(az)) = (tokens.as_ref(), az.as_ref()) {
            if path == az {
                return Arc::clone(source);
            }
        }
        let source: Arc<dyn TokenSource> = Arc::new(AzCliTokenSource::new(
            az.clone(),
            Arc::clone(&self.backends.runner),
            Arc::clone(&self.backends.clock),
        ));
        *tokens = az.map(|az| (az, Arc::clone(&source)));
        source
    }

    /// Setup state and the bound account; probes once if never probed.
    pub async fn report(self: &Arc<Self>) -> Report {
        let probed = self.lock().probed.is_some();
        let setup = if probed {
            self.view()
        } else {
            self.check().await
        };
        let inner = self.lock();
        let account = match (&inner.binding.identity, &inner.binding.organization) {
            (Some(bound), Some(organization)) => {
                use crate::provider::ConnectionError as E;
                let reason = match &setup.account {
                    AccountView::Connected { identity } if identity.id == bound.id => None,
                    AccountView::Connected { .. } | AccountView::WrongIdentity { .. } => {
                        Some(E::WrongIdentity)
                    }
                    AccountView::Offline => Some(E::Network),
                    AccountView::Failed => Some(E::BrokenCli),
                    AccountView::Unavailable => Some(match setup.cli {
                        CliView::Missing | CliView::TooOld { .. } => E::MissingCli,
                        CliView::Failed => E::BrokenCli,
                        _ => E::Configuration,
                    }),
                    _ => Some(E::SignedOut),
                };
                Some(RepositoryAccount {
                    state: if reason.is_none() {
                        "connected"
                    } else {
                        "reconnect_required"
                    },
                    provider: "azure_devops",
                    account_id: bound.id.clone(),
                    login: bound.display_name.clone(),
                    organization: organization.clone(),
                    connection_generation: 0,
                    reason,
                })
            }
            _ => None,
        };
        Report { setup, account }
    }

    /// The connected account, probing first unless already connected.
    /// Never interactive.
    pub async fn connected(
        self: &Arc<Self>,
    ) -> Result<Connected, crate::provider::ConnectionError> {
        use crate::provider::ConnectionError as E;
        let connected = |inner: &Inner| {
            matches!(
                inner.probed.as_ref().map(|probed| &probed.account),
                Some(AccountView::Connected { .. })
            )
        };
        if !connected(&self.lock()) {
            self.check().await;
        }
        let inner = self.lock();
        let probed = inner.probed.as_ref().ok_or(E::Configuration)?;
        match (
            &probed.account,
            &probed.tenant,
            &inner.binding.organization,
            &inner.binding.identity,
        ) {
            (
                AccountView::Connected { identity },
                Some(tenant),
                Some(organization),
                Some(bound),
            ) if identity.id == bound.id => Ok(Connected {
                organization: organization.clone(),
                tenant: tenant.clone(),
                identity: identity.clone(),
            }),
            (AccountView::Connected { .. } | AccountView::WrongIdentity { .. }, ..) => {
                Err(E::WrongIdentity)
            }
            (AccountView::Offline, ..) => Err(E::Network),
            (AccountView::Failed, ..) => Err(E::BrokenCli),
            (AccountView::Unavailable, ..) => Err(match probed.cli {
                CliView::Missing | CliView::TooOld { .. } => E::MissingCli,
                CliView::Failed => E::BrokenCli,
                _ => E::Configuration,
            }),
            _ => Err(E::SignedOut),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn binding(&self) -> Binding {
        self.lock().binding.clone()
    }

    pub fn view(&self) -> SetupView {
        let inner = self.lock();
        let probed = inner.probed.as_ref();
        let job = inner.job.as_ref().map(|job| job.kind);
        let organization = match (&inner.binding.organization, probed) {
            (None, _) => OrganizationView::Unset,
            (Some(_), Some(probed)) => probed.organization.clone(),
            (Some(name), None) => OrganizationView::Unknown { name: name.clone() },
        };
        let cli = probed.map_or(CliView::Unknown, |probed| probed.cli.clone());
        SetupView {
            cli: if job == Some(JobKind::Install) {
                CliView::Installing {
                    version: match cli {
                        CliView::TooOld { version } | CliView::Ready { version } => Some(version),
                        _ => None,
                    },
                }
            } else {
                cli
            },
            minimum: az_cli::MIN_VERSION.to_string(),
            installer: probed.map_or(Installer::Page, |probed| probed.installer),
            organization,
            account: if job == Some(JobKind::SignIn) {
                AccountView::SigningIn
            } else {
                probed.map_or(AccountView::Unavailable, |probed| probed.account.clone())
            },
            checking: inner.probing > 0,
        }
    }

    /// Re-detects everything without user interaction.
    pub async fn check(self: &Arc<Self>) -> SetupView {
        let (generation, binding) = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.probing += 1;
            (inner.generation, inner.binding.clone())
        };
        let probed = probe(&self.backends, &binding).await;
        let mut inner = self.lock();
        inner.probing -= 1;
        if inner.generation == generation && inner.binding == binding {
            let mut probed = probed;
            if let Some(identity) = &probed.bind {
                let next = Binding {
                    identity: Some(identity.clone()),
                    ..binding
                };
                if (self.backends.persist)(&next).is_ok() {
                    inner.binding = next;
                    self.reset_tokens();
                } else if binding.identity.is_none() {
                    // Never show an unbound identity as connected; the next
                    // probe retries the write.
                    probed.account = AccountView::Failed;
                }
            }
            inner.probed = Some(probed);
        }
        drop(inner);
        self.view()
    }

    pub async fn set_organization(
        self: &Arc<Self>,
        organization: &str,
    ) -> Result<SetupView, SetupError> {
        let organization = organization.trim();
        if !tenant::valid_organization(organization) {
            return Err(SetupError::InvalidOrganization);
        }
        {
            let mut inner = self.lock();
            let unchanged = inner
                .binding
                .organization
                .as_deref()
                .is_some_and(|saved| saved.eq_ignore_ascii_case(organization));
            if !unchanged {
                // The identity belongs to the previous organization.
                let next = Binding {
                    organization: Some(organization.to_owned()),
                    identity: None,
                };
                (self.backends.persist)(&next).map_err(|_| SetupError::Storage)?;
                inner.binding = next;
                self.reset_tokens();
                inner.probed = None;
                inner.generation += 1;
            }
        }
        Ok(self.check().await)
    }

    /// Wrong identity only: binds the shown signed-in identity in place of
    /// the bound one. Azure CLI is untouched (never `az logout`).
    pub async fn use_signed_in_account(self: &Arc<Self>) -> Result<SetupView, SetupError> {
        {
            let mut inner = self.lock();
            let Some(Probed {
                account: AccountView::WrongIdentity { identity, .. },
                ..
            }) = inner.probed.as_ref()
            else {
                return Err(SetupError::NotReady);
            };
            let next = Binding {
                identity: Some(identity.clone()),
                ..inner.binding.clone()
            };
            (self.backends.persist)(&next).map_err(|_| SetupError::Storage)?;
            inner.binding = next;
            self.reset_tokens();
            inner.generation += 1;
        }
        Ok(self.check().await)
    }

    /// User-initiated: installs or updates Azure CLI with the platform
    /// package manager, else opens the install page.
    pub fn install(self: &Arc<Self>) -> Result<SetupView, SetupError> {
        let upgrade = {
            let inner = self.lock();
            if inner.job.is_some() {
                return Err(SetupError::Busy);
            }
            match inner.probed.as_ref().map(|probed| &probed.cli) {
                Some(CliView::Ready { .. } | CliView::Failed) => return Err(SetupError::NotReady),
                Some(CliView::TooOld { .. }) => true,
                _ => false,
            }
        };
        match install_plan(
            self.backends.platform,
            upgrade,
            (self.backends.locate)(Program::Homebrew),
            (self.backends.locate)(Program::Winget),
        ) {
            InstallPlan::Page => {
                if !(self.backends.open_page)(INSTALL_PAGE) {
                    return Err(SetupError::BrowserOpen);
                }
            }
            InstallPlan::Run {
                installer,
                program,
                args,
            } => {
                let env = match installer {
                    Installer::Homebrew => &HOMEBREW_ENV,
                    _ => &WINGET_ENV,
                };
                self.start(
                    JobKind::Install,
                    program,
                    args.into_iter().map(str::to_owned).collect(),
                    env,
                    INSTALL_TIMEOUT,
                )?;
            }
        }
        Ok(self.view())
    }

    /// User-initiated after confirmation: `az login` for the organization's
    /// tenant, then a re-probe. Never called from background work.
    pub fn sign_in(self: &Arc<Self>) -> Result<SetupView, SetupError> {
        let tenant = {
            let inner = self.lock();
            if inner.job.is_some() {
                return Err(SetupError::Busy);
            }
            match inner.probed.as_ref() {
                Some(Probed {
                    cli: CliView::Ready { .. },
                    tenant: Some(tenant),
                    ..
                }) => tenant.clone(),
                _ => return Err(SetupError::NotReady),
            }
        };
        let program = (self.backends.locate)(Program::Az).ok_or(SetupError::NotReady)?;
        self.start(
            JobKind::SignIn,
            program,
            sign_in_args(&tenant),
            &az_cli::ENV,
            SIGN_IN_TIMEOUT,
        )?;
        Ok(self.view())
    }

    /// Stops the running install or sign-in (killing only its child).
    pub async fn cancel(self: &Arc<Self>) -> SetupView {
        let job = self.lock().job.take();
        if let Some(job) = job {
            job.task.abort();
            // Dropping the run kills its child before the re-probe.
            let _ = job.task.await;
        }
        self.check().await
    }

    fn start(
        self: &Arc<Self>,
        kind: JobKind,
        program: PathBuf,
        args: Vec<String>,
        env: &'static EnvPolicy,
        timeout: Duration,
    ) -> Result<(), SetupError> {
        let mut inner = self.lock();
        if inner.job.is_some() {
            return Err(SetupError::Busy);
        }
        inner.next_job += 1;
        let id = inner.next_job;
        let setup = Arc::clone(self);
        let task = tokio::spawn(async move {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            // Output is discarded unread; the re-probe decides the state.
            let _ = setup
                .backends
                .runner
                .run(Invocation {
                    program: Path::new(&program),
                    args: &args,
                    env,
                    limits: Limits {
                        timeout,
                        output_bytes: OUTPUT_LIMIT,
                    },
                })
                .await;
            {
                let mut inner = setup.lock();
                if inner.job.as_ref().is_none_or(|job| job.id != id) {
                    return;
                }
                inner.job = None;
            }
            setup.check().await;
        });
        inner.job = Some(Job { id, kind, task });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
