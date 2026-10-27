use super::*;
use crate::azure_devops::connection::ConnectionResponse;
use crate::azure_devops::tenant::{HeadResponse, NetworkError};
use crate::azure_devops::token_source::AccessToken;
use crate::cli_process::{Output, RunError};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroizing;

const TENANT: &str = "00000000-0000-0000-0000-00000000000a";
const AVERY: &str = "00000000-0000-0000-0000-000000000001";
const BLAKE: &str = "00000000-0000-0000-0000-000000000002";

#[derive(Clone)]
enum Token {
    Ok,
    Fail(&'static str),
}

/// Mutable fixture world shared by the fakes.
struct World {
    /// `None`: `az` not installed.
    version: Option<&'static str>,
    /// A located `az` whose `version` run fails this way.
    version_failure: Option<Result<Output, RunError>>,
    version_after_install: Option<&'static str>,
    token: Token,
    token_after_login: Token,
    /// Status and tenant header per organization; missing → network error.
    organizations: Vec<(&'static str, u16, Option<&'static str>)>,
    /// `None`: network error.
    connection: Option<(u16, String)>,
    homebrew: bool,
    winget: bool,
    hold: bool,
    runs: Vec<(String, Vec<String>, Duration, Vec<&'static str>)>,
    saved: Vec<Binding>,
    persist_fails: bool,
    opened: Vec<String>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            version: Some("2.90.0"),
            version_failure: None,
            version_after_install: Some("2.90.0"),
            token: Token::Ok,
            token_after_login: Token::Ok,
            organizations: vec![("contoso", 401, Some(TENANT))],
            connection: Some((200, identity_body(AVERY, "Avery Example"))),
            homebrew: true,
            winget: false,
            hold: false,
            runs: Vec::new(),
            saved: Vec::new(),
            persist_fails: false,
            opened: Vec::new(),
        }
    }
}

fn first_name(name: &str) -> String {
    name.split(' ').next().unwrap_or_default().to_lowercase()
}

fn identity_body(id: &str, name: &str) -> String {
    format!(
        r#"{{"authenticatedUser":{{"id":"{id}","providerDisplayName":"{name}","properties":{{"Account":{{"$value":"{}@contoso.example"}}}}}}}}"#,
        first_name(name)
    )
}

fn identity(id: &str, name: &str) -> Identity {
    Identity {
        id: id.into(),
        display_name: name.into(),
        unique_name: Some(format!("{}@contoso.example", first_name(name))),
    }
}

type Shared = Arc<Mutex<World>>;

struct Fake {
    world: Shared,
    release: Arc<tokio::sync::Semaphore>,
    dropped: Arc<AtomicBool>,
}

fn output(code: i32, stdout: String, stderr: &str) -> Output {
    Output {
        code: Some(code),
        stdout: Zeroizing::new(stdout.into_bytes()),
        stderr: Zeroizing::new(stderr.as_bytes().to_vec()),
    }
}

/// Records a long-running child dropped before it finished (killed).
struct DropFlag(Arc<AtomicBool>, bool);

impl Drop for DropFlag {
    fn drop(&mut self) {
        if !self.1 {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl CommandRunner for Fake {
    async fn run(&self, invocation: Invocation<'_>) -> Result<Output, RunError> {
        let program = invocation.program.to_string_lossy().into_owned();
        let args: Vec<String> = invocation.args.iter().map(|a| a.to_string()).collect();
        let hold = {
            let mut world = self.world.lock().unwrap();
            world.runs.push((
                program,
                args.clone(),
                invocation.limits.timeout,
                invocation.env.remove.to_vec(),
            ));
            world.hold
        };
        let action = args.first().map(String::as_str);
        if matches!(action, Some("login" | "install" | "upgrade")) {
            let mut flag = DropFlag(Arc::clone(&self.dropped), false);
            if hold {
                self.release.acquire().await.unwrap().forget();
            }
            flag.1 = true;
            let mut world = self.world.lock().unwrap();
            if action == Some("login") {
                world.token = world.token_after_login.clone();
            } else {
                world.version = world.version_after_install;
            }
            return Ok(output(0, String::new(), ""));
        }
        let mut world = self.world.lock().unwrap();
        if action == Some("version") {
            if let Some(failure) = world.version_failure.take() {
                return failure;
            }
        }
        match action {
            Some("version") => match world.version {
                Some(version) => Ok(output(
                    0,
                    format!(r#"{{"azure-cli":"{version}","extensions":{{}}}}"#),
                    "",
                )),
                None => Err(RunError::NotFound),
            },
            Some("account") => match &world.token {
                Token::Ok => Ok(output(
                    0,
                    r#"{"accessToken":"fixture.token","expires_on":4102444800,"tokenType":"Bearer"}"#
                        .into(),
                    "",
                )),
                Token::Fail(stderr) => Ok(output(1, String::new(), stderr)),
            },
            _ => Err(RunError::Failed),
        }
    }
}

#[async_trait]
impl HeadTransport for Fake {
    async fn head(&self, url: &str) -> Result<HeadResponse, NetworkError> {
        let world = self.world.lock().unwrap();
        world
            .organizations
            .iter()
            .find(|(name, ..)| url == format!("https://dev.azure.com/{name}"))
            .map(|(_, status, tenant)| HeadResponse {
                status: *status,
                resource_tenant: tenant.map(str::to_owned),
            })
            .ok_or(NetworkError)
    }
}

#[async_trait]
impl ConnectionTransport for Fake {
    async fn connection_data(
        &self,
        _organization: &str,
        token: &AccessToken,
    ) -> Result<ConnectionResponse, NetworkError> {
        assert_eq!(token.secret(), "fixture.token");
        let world = self.world.lock().unwrap();
        world
            .connection
            .clone()
            .map(|(status, body)| ConnectionResponse {
                status,
                body: body.into_bytes(),
            })
            .ok_or(NetworkError)
    }
}

struct Harness {
    world: Shared,
    release: Arc<tokio::sync::Semaphore>,
    dropped: Arc<AtomicBool>,
    setup: Arc<Setup>,
}

impl Harness {
    fn with(platform: Platform, world: World, binding: Binding) -> Self {
        let world = Arc::new(Mutex::new(world));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let fake = Arc::new(Fake {
            world: Arc::clone(&world),
            release: Arc::clone(&release),
            dropped: Arc::clone(&dropped),
        });
        let locate_world = Arc::clone(&world);
        let open_world = Arc::clone(&world);
        let persist_world = Arc::clone(&world);
        let setup = Setup::new(
            Backends {
                runner: fake.clone(),
                head: fake.clone(),
                connection: fake,
                clock: Arc::new(|| 1_000),
                platform,
                locate: Arc::new(move |program| {
                    let world = locate_world.lock().unwrap();
                    match program {
                        Program::Az => world.version.map(|_| PathBuf::from("/fixture/az")),
                        Program::Homebrew => world.homebrew.then(|| PathBuf::from("/fixture/brew")),
                        Program::Winget => {
                            world.winget.then(|| PathBuf::from("/fixture/winget.exe"))
                        }
                    }
                }),
                open_page: Arc::new(move |url| {
                    open_world.lock().unwrap().opened.push(url.into());
                    true
                }),
                persist: Arc::new(move |binding| {
                    let mut world = persist_world.lock().unwrap();
                    if world.persist_fails {
                        return Err(());
                    }
                    world.saved.push(binding.clone());
                    Ok(())
                }),
            },
            binding,
        );
        Self {
            world,
            release,
            dropped,
            setup,
        }
    }

    fn new(world: World) -> Self {
        Self::with(Platform::MacOs, world, contoso())
    }

    fn world(&self) -> MutexGuard<'_, World> {
        self.world.lock().unwrap()
    }

    fn actions(&self) -> Vec<String> {
        self.world()
            .runs
            .iter()
            .map(|(_, args, ..)| args.first().cloned().unwrap_or_default())
            .collect()
    }

    async fn settle(&self) -> SetupView {
        for _ in 0..400 {
            let view = self.setup.view();
            if !view.checking
                && !matches!(view.cli, CliView::Installing { .. })
                && view.account != AccountView::SigningIn
            {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("setup did not settle");
    }
}

fn contoso() -> Binding {
    Binding {
        organization: Some("contoso".into()),
        identity: None,
    }
}

fn known() -> OrganizationView {
    OrganizationView::TenantKnown {
        name: "contoso".into(),
        tenant: TENANT.into(),
    }
}

const SIGNED_OUT: &str = "ERROR: Please run 'az login' to setup account.";
const MFA: &str = "ERROR: AADSTS50076: multi-factor authentication required.";

#[tokio::test]
async fn reports_unknown_before_the_first_probe() {
    let harness = Harness::new(World::default());
    let view = harness.setup.view();
    assert_eq!(view.cli, CliView::Unknown);
    assert_eq!(
        view.organization,
        OrganizationView::Unknown {
            name: "contoso".into()
        }
    );
    assert_eq!(view.account, AccountView::Unavailable);
    assert_eq!(view.minimum, "2.54.0");
    assert!(harness.actions().is_empty());
}

#[tokio::test]
async fn detects_missing_old_and_ready_azure_cli() {
    for (version, expected) in [
        (None, CliView::Missing),
        (
            Some("2.53.1"),
            CliView::TooOld {
                version: "2.53.1".into(),
            },
        ),
        (
            Some("2.90.0"),
            CliView::Ready {
                version: "2.90.0".into(),
            },
        ),
    ] {
        let harness = Harness::new(World {
            version,
            ..World::default()
        });
        let view = harness.setup.check().await;
        assert_eq!(view.cli, expected);
        assert_eq!(view.organization, known(), "{version:?}");
        if version != Some("2.90.0") {
            assert_eq!(view.account, AccountView::Unavailable);
        }
    }
}

#[tokio::test]
async fn a_located_cli_whose_version_run_fails_is_failed_not_missing() {
    for failure in [
        Err(RunError::Timeout),
        Ok(output(2, String::new(), "ERROR: broken")),
        Ok(output(0, "not json".into(), "")),
    ] {
        let harness = Harness::new(World {
            version_failure: Some(failure),
            ..World::default()
        });
        let view = harness.setup.check().await;
        assert_eq!(view.cli, CliView::Failed);
        assert_eq!(view.account, AccountView::Unavailable);
        assert_eq!(harness.setup.install(), Err(SetupError::NotReady));
        assert!(!harness.actions().iter().any(|action| action == "install"));
        assert_eq!(
            harness.setup.check().await.cli,
            CliView::Ready {
                version: "2.90.0".into()
            }
        );
    }
}

#[tokio::test]
async fn resolves_organization_states() {
    let named = |name: &str| name.to_string();
    for (organizations, expected) in [
        (
            vec![("contoso", 404, None)],
            OrganizationView::NotFound {
                name: named("contoso"),
            },
        ),
        (
            vec![("contoso", 401, None)],
            OrganizationView::TenantUnknown {
                name: named("contoso"),
            },
        ),
        (
            vec![("contoso", 401, Some("00000000-0000-0000-0000-000000000000"))],
            OrganizationView::TenantUnknown {
                name: named("contoso"),
            },
        ),
        (
            vec![("contoso", 503, None)],
            OrganizationView::Offline {
                name: named("contoso"),
            },
        ),
        (
            vec![],
            OrganizationView::Offline {
                name: named("contoso"),
            },
        ),
        (vec![("contoso", 401, Some(TENANT))], known()),
    ] {
        let tenant_known = expected == known();
        let harness = Harness::new(World {
            organizations,
            ..World::default()
        });
        let view = harness.setup.check().await;
        assert_eq!(view.organization, expected);
        if !tenant_known {
            assert_eq!(view.account, AccountView::Unavailable);
        }
    }
    let unset = Harness::with(Platform::MacOs, World::default(), Binding::default());
    let view = unset.setup.check().await;
    assert_eq!(view.organization, OrganizationView::Unset);
    assert_eq!(view.account, AccountView::Unavailable);
}

#[tokio::test]
async fn maps_account_states_without_interaction() {
    type Case = (Token, Option<(u16, String)>, AccountView);
    let cases: Vec<Case> = vec![
        (Token::Fail(SIGNED_OUT), None, AccountView::SignedOut),
        (Token::Fail(MFA), None, AccountView::InteractionRequired),
        (
            Token::Fail("ERROR: AADSTS500213: cross-tenant access policy."),
            None,
            AccountView::TenantAccessBlocked,
        ),
        (
            Token::Fail("ERROR: Max retries exceeded with url"),
            None,
            AccountView::Offline,
        ),
        (
            Token::Fail("ERROR: something new"),
            None,
            AccountView::Failed,
        ),
        (
            Token::Ok,
            Some((401, String::new())),
            AccountView::SignedOut,
        ),
        (Token::Ok, None, AccountView::Offline),
        (Token::Ok, Some((500, String::new())), AccountView::Failed),
        (Token::Ok, Some((200, "{".into())), AccountView::Failed),
    ];
    for (token, connection, expected) in cases {
        let harness = Harness::new(World {
            token,
            connection,
            ..World::default()
        });
        let view = harness.setup.check().await;
        assert_eq!(view.account, expected);
        assert!(harness.world().saved.is_empty());
        let actions = harness.actions();
        assert!(
            actions
                .iter()
                .all(|action| action == "version" || action == "account"),
            "{actions:?}"
        );
    }
}

#[tokio::test]
async fn binds_the_connection_data_identity_and_rejects_another_one() {
    let harness = Harness::new(World::default());
    let view = harness.setup.check().await;
    assert_eq!(
        view.account,
        AccountView::Connected {
            identity: identity(AVERY, "Avery Example")
        }
    );
    let bound = Binding {
        organization: Some("contoso".into()),
        identity: Some(identity(AVERY, "Avery Example")),
    };
    assert_eq!(harness.world().saved, std::slice::from_ref(&bound));
    assert_eq!(harness.setup.binding(), bound);
    harness.setup.check().await;
    assert_eq!(harness.world().saved.len(), 1);

    harness.world().connection = Some((200, identity_body(BLAKE, "Blake Example")));
    let view = harness.setup.check().await;
    assert_eq!(
        view.account,
        AccountView::WrongIdentity {
            identity: identity(BLAKE, "Blake Example"),
            expected: identity(AVERY, "Avery Example"),
        }
    );
    assert_eq!(harness.setup.binding(), bound);

    // Only an explicit Use rebinds, to the identity shown; never `az logout`.
    let view = harness.setup.use_signed_in_account().await.unwrap();
    assert_eq!(
        view.account,
        AccountView::Connected {
            identity: identity(BLAKE, "Blake Example")
        }
    );
    assert_eq!(
        harness.setup.binding().identity,
        Some(identity(BLAKE, "Blake Example"))
    );
    assert!(!harness.actions().iter().any(|action| action == "logout"));
    assert_eq!(
        harness.setup.use_signed_in_account().await,
        Err(SetupError::NotReady)
    );
    assert_eq!(harness.world().saved.len(), 2);
}

#[tokio::test]
async fn an_unsaved_first_binding_is_never_shown_connected() {
    let harness = Harness::new(World {
        persist_fails: true,
        ..World::default()
    });
    let view = harness.setup.check().await;
    assert_eq!(view.account, AccountView::Failed);
    assert_eq!(harness.setup.binding().identity, None);

    harness.world().persist_fails = false;
    let view = harness.setup.check().await;
    assert_eq!(
        view.account,
        AccountView::Connected {
            identity: identity(AVERY, "Avery Example")
        }
    );
    harness.world().connection = Some((200, identity_body(BLAKE, "Blake Example")));
    assert_eq!(
        harness.setup.check().await.account,
        AccountView::WrongIdentity {
            identity: identity(BLAKE, "Blake Example"),
            expected: identity(AVERY, "Avery Example"),
        }
    );
}

#[tokio::test]
async fn organization_changes_validate_and_drop_the_previous_identity() {
    let harness = Harness::new(World {
        organizations: vec![
            ("contoso", 401, Some(TENANT)),
            ("fabrikam", 401, Some(TENANT)),
        ],
        ..World::default()
    });
    harness.setup.check().await;
    for invalid in ["", "a/b", "-x", "a b"] {
        assert_eq!(
            harness.setup.set_organization(invalid).await,
            Err(SetupError::InvalidOrganization)
        );
    }
    assert_eq!(harness.world().saved.len(), 1);
    harness.world().connection = Some((200, identity_body(BLAKE, "Blake Example")));
    let view = harness.setup.set_organization(" fabrikam ").await.unwrap();
    assert_eq!(
        view.organization,
        OrganizationView::TenantKnown {
            name: "fabrikam".into(),
            tenant: TENANT.into()
        }
    );
    assert_eq!(
        view.account,
        AccountView::Connected {
            identity: identity(BLAKE, "Blake Example")
        }
    );
    assert_eq!(
        harness.world().saved[1],
        Binding {
            organization: Some("fabrikam".into()),
            identity: None
        }
    );

    harness.world().persist_fails = true;
    assert_eq!(
        harness.setup.set_organization("contoso").await,
        Err(SetupError::Storage)
    );
    assert_eq!(
        harness.setup.binding().organization.as_deref(),
        Some("fabrikam")
    );
}

#[test]
fn plans_installs_per_platform() {
    let brew = || Some(PathBuf::from("/fixture/brew"));
    let winget = || Some(PathBuf::from("/fixture/winget.exe"));
    assert_eq!(
        install_plan(Platform::MacOs, false, brew(), None),
        InstallPlan::Run {
            installer: Installer::Homebrew,
            program: PathBuf::from("/fixture/brew"),
            args: vec!["install", "azure-cli"],
        }
    );
    assert_eq!(
        install_plan(Platform::MacOs, true, brew(), None),
        InstallPlan::Run {
            installer: Installer::Homebrew,
            program: PathBuf::from("/fixture/brew"),
            args: vec!["upgrade", "azure-cli"],
        }
    );
    assert_eq!(
        install_plan(Platform::Windows, false, None, winget()),
        InstallPlan::Run {
            installer: Installer::Winget,
            program: PathBuf::from("/fixture/winget.exe"),
            args: vec![
                "install",
                "-e",
                "--id",
                "Microsoft.AzureCLI",
                "--source",
                "winget",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ],
        }
    );
    assert_eq!(
        install_plan(Platform::Windows, true, None, winget()),
        InstallPlan::Run {
            installer: Installer::Winget,
            program: PathBuf::from("/fixture/winget.exe"),
            args: vec![
                "upgrade",
                "-e",
                "--id",
                "Microsoft.AzureCLI",
                "--source",
                "winget",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ],
        }
    );
    for (platform, homebrew, winget) in [
        (Platform::MacOs, None, winget()),
        (Platform::Windows, brew(), None),
        (Platform::Other, brew(), winget()),
    ] {
        assert_eq!(
            install_plan(platform, false, homebrew, winget),
            InstallPlan::Page
        );
    }
}

#[tokio::test]
async fn installs_as_a_tracked_child_then_redetects() {
    let harness = Harness::new(World {
        version: None,
        hold: true,
        ..World::default()
    });
    let view = harness.setup.check().await;
    assert_eq!(
        (view.cli, view.installer),
        (CliView::Missing, Installer::Homebrew)
    );
    let view = harness.setup.install().unwrap();
    assert_eq!(view.cli, CliView::Installing { version: None });
    assert_eq!(harness.setup.install(), Err(SetupError::Busy));
    assert_eq!(harness.setup.sign_in(), Err(SetupError::Busy));
    harness.release.add_permits(1);
    let view = harness.settle().await;
    assert_eq!(
        view.cli,
        CliView::Ready {
            version: "2.90.0".into()
        }
    );
    let world = harness.world();
    let (program, args, timeout, _) = world
        .runs
        .iter()
        .find(|(_, args, ..)| args[0] == "install")
        .unwrap();
    assert_eq!(program, "/fixture/brew");
    assert_eq!(args, &["install", "azure-cli"]);
    assert_eq!(*timeout, INSTALL_TIMEOUT);
}

#[tokio::test]
async fn installs_with_winget_on_windows() {
    let harness = Harness::with(
        Platform::Windows,
        World {
            version: None,
            homebrew: false,
            winget: true,
            ..World::default()
        },
        contoso(),
    );
    assert_eq!(harness.setup.check().await.installer, Installer::Winget);
    harness.setup.install().unwrap();
    harness.settle().await;
    let world = harness.world();
    let (program, args, ..) = world
        .runs
        .iter()
        .find(|(_, args, ..)| args[0] == "install")
        .unwrap();
    assert_eq!(program, "/fixture/winget.exe");
    assert_eq!(
        args,
        &[
            "install",
            "-e",
            "--id",
            "Microsoft.AzureCLI",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
        ]
    );
}

#[tokio::test]
async fn updates_an_old_cli_and_cancel_kills_the_child() {
    let harness = Harness::new(World {
        version: Some("2.53.1"),
        hold: true,
        ..World::default()
    });
    harness.setup.check().await;
    let view = harness.setup.install().unwrap();
    assert_eq!(
        view.cli,
        CliView::Installing {
            version: Some("2.53.1".into())
        }
    );
    tokio::task::yield_now().await;
    assert!(harness.actions().contains(&"upgrade".to_string()));
    let view = harness.setup.cancel().await;
    assert!(harness.dropped.load(Ordering::SeqCst));
    assert_eq!(
        view.cli,
        CliView::TooOld {
            version: "2.53.1".into()
        }
    );
    harness.setup.install().unwrap();
    harness.release.add_permits(1);
    assert_eq!(
        harness.settle().await.cli,
        CliView::Ready {
            version: "2.90.0".into()
        }
    );
}

#[tokio::test]
async fn opens_the_install_page_without_a_package_manager() {
    let harness = Harness::new(World {
        version: None,
        homebrew: false,
        ..World::default()
    });
    let view = harness.setup.check().await;
    assert_eq!(view.installer, Installer::Page);
    let view = harness.setup.install().unwrap();
    assert_eq!(view.cli, CliView::Missing);
    assert_eq!(harness.world().opened, [INSTALL_PAGE]);
    assert!(!harness.actions().iter().any(|action| action == "install"));
}

#[tokio::test]
async fn ready_cli_is_not_reinstalled() {
    let harness = Harness::new(World::default());
    harness.setup.check().await;
    assert_eq!(harness.setup.install(), Err(SetupError::NotReady));
}

#[tokio::test]
async fn signs_in_to_the_organization_tenant_then_reprobes() {
    let harness = Harness::new(World {
        token: Token::Fail(SIGNED_OUT),
        hold: true,
        ..World::default()
    });
    assert_eq!(harness.setup.sign_in(), Err(SetupError::NotReady));
    let view = harness.setup.check().await;
    assert_eq!(view.account, AccountView::SignedOut);
    let view = harness.setup.sign_in().unwrap();
    assert_eq!(view.account, AccountView::SigningIn);
    assert_eq!(harness.setup.sign_in(), Err(SetupError::Busy));
    harness.release.add_permits(1);
    let view = harness.settle().await;
    assert_eq!(
        view.account,
        AccountView::Connected {
            identity: identity(AVERY, "Avery Example")
        }
    );
    let world = harness.world();
    let (program, args, timeout, removed) = world
        .runs
        .iter()
        .find(|(_, args, ..)| args[0] == "login")
        .unwrap();
    assert_eq!(program, "/fixture/az");
    assert_eq!(
        args,
        &[
            "login",
            "--tenant",
            TENANT,
            "--allow-no-subscriptions",
            "--scope",
            "499b84ac-1321-427f-aa17-267ca6975798/.default",
            "-o",
            "none"
        ]
    );
    // Passable to `az.cmd` on Windows.
    assert!(args.iter().all(|arg| crate::cli_process::batch_safe(arg)));
    assert_eq!(*timeout, Duration::from_secs(300));
    assert!(removed.contains(&"COPILOT_AGENT_SESSION_ID"));
    assert_eq!(
        world.runs.iter().filter(|run| run.1[0] == "login").count(),
        1
    );
}

#[tokio::test]
async fn cancelled_sign_in_kills_the_child_and_reprobes() {
    let harness = Harness::new(World {
        token: Token::Fail(MFA),
        hold: true,
        ..World::default()
    });
    harness.setup.check().await;
    harness.setup.sign_in().unwrap();
    tokio::task::yield_now().await;
    let view = harness.setup.cancel().await;
    assert!(harness.dropped.load(Ordering::SeqCst));
    assert_eq!(view.account, AccountView::InteractionRequired);
}

#[tokio::test]
async fn sign_in_needs_a_ready_cli_and_known_tenant() {
    for world in [
        World {
            version: Some("2.53.1"),
            ..World::default()
        },
        World {
            organizations: vec![("contoso", 404, None)],
            ..World::default()
        },
    ] {
        let harness = Harness::new(world);
        harness.setup.check().await;
        assert_eq!(harness.setup.sign_in(), Err(SetupError::NotReady));
        assert!(!harness.actions().iter().any(|action| action == "login"));
    }
}

#[test]
fn views_serialize_as_tagged_values() {
    let view = SetupView {
        cli: CliView::Ready {
            version: "2.90.0".into(),
        },
        minimum: "2.54.0".into(),
        installer: Installer::Homebrew,
        organization: known(),
        account: AccountView::WrongIdentity {
            identity: identity(BLAKE, "Blake Example"),
            expected: identity(AVERY, "Avery Example"),
        },
        checking: false,
    };
    let value = serde_json::to_value(view).unwrap();
    assert_eq!(value["cli"]["state"], "ready");
    assert_eq!(value["organization"]["state"], "tenant_known");
    assert_eq!(value["account"]["state"], "wrong_identity");
    assert_eq!(value["account"]["expected"]["id"], AVERY);
    assert_eq!(value["installer"], "homebrew");
    assert_eq!(
        String::from(SetupError::InvalidOrganization),
        "invalid_organization"
    );
}

#[tokio::test]
async fn reads_use_the_connected_bound_account_without_interaction() {
    use crate::provider::ConnectionError as E;
    let harness = Harness::new(World::default());
    // Never probed: `connected` probes first.
    let connected = harness.setup.connected().await.unwrap();
    assert_eq!(connected.organization, "contoso");
    assert_eq!(connected.tenant.as_str(), TENANT);
    assert_eq!(connected.identity, identity(AVERY, "Avery Example"));
    let probes = harness.actions().len();
    harness.setup.connected().await.unwrap();
    assert_eq!(
        harness.actions().len(),
        probes,
        "connected is not re-probed"
    );

    harness.world().connection = Some((200, identity_body(BLAKE, "Blake Example")));
    harness.setup.check().await;
    assert_eq!(harness.setup.connected().await, Err(E::WrongIdentity));
    harness.world().token = Token::Fail(SIGNED_OUT);
    assert_eq!(harness.setup.connected().await, Err(E::SignedOut));
    harness.world().token = Token::Fail(MFA);
    assert_eq!(harness.setup.connected().await, Err(E::SignedOut));
    harness.world().version = None;
    assert_eq!(harness.setup.connected().await, Err(E::MissingCli));
    assert!(!harness
        .actions()
        .iter()
        .any(|action| matches!(action.as_str(), "login" | "install" | "upgrade")));

    let unset = Harness::with(Platform::MacOs, World::default(), Binding::default());
    assert_eq!(unset.setup.connected().await, Err(E::Configuration));
}

#[tokio::test]
async fn reports_setup_state_with_the_bound_account() {
    let unbound = Harness::with(Platform::MacOs, World::default(), Binding::default());
    let report = unbound.setup.report().await;
    assert_eq!(report.setup.organization, OrganizationView::Unset);
    assert_eq!(report.account, None);

    let harness = Harness::new(World::default());
    let report = harness.setup.report().await;
    assert_eq!(report.setup.organization, known());
    let account = report.account.unwrap();
    assert_eq!(
        serde_json::to_value(&account).unwrap(),
        serde_json::json!({"state":"connected","provider":"azure_devops","account_id":AVERY,
            "login":"Avery Example","organization":"contoso","connection_generation":0})
    );

    harness.world().token = Token::Fail(SIGNED_OUT);
    harness.setup.check().await;
    let account = harness.setup.report().await.account.unwrap();
    assert_eq!(account.state, "reconnect_required");
    assert_eq!(
        account.reason,
        Some(crate::provider::ConnectionError::SignedOut)
    );
}

#[tokio::test]
async fn reads_share_one_token_cache_per_located_cli() {
    let harness = Harness::new(World::default());
    let first = harness.setup.token_source();
    assert!(Arc::ptr_eq(&first, &harness.setup.token_source()));
    let tenant = TenantId::parse(TENANT).unwrap();
    first.token(&tenant).await.unwrap();
    harness.setup.token_source().token(&tenant).await.unwrap();
    assert_eq!(
        harness
            .actions()
            .iter()
            .filter(|action| *action == "account")
            .count(),
        1
    );
}
