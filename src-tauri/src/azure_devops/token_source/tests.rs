use super::*;
use crate::cli_process::{Output, RunError};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

const TENANT: &str = "00000000-0000-0000-0000-000000000001";

#[derive(Clone)]
enum Step {
    Token {
        secret: &'static str,
        expires_on: i64,
    },
    Version(&'static str),
    Fail {
        code: i32,
        stderr: &'static str,
    },
    Raw(&'static str),
    Err(RunError),
}

#[derive(Default)]
struct Script {
    versions: Vec<Step>,
    tokens: Vec<Step>,
}

/// Arguments and removed environment names of each run.
type Invocations = Arc<Mutex<Vec<(Vec<String>, Vec<&'static str>)>>>;

#[derive(Clone, Default)]
struct FakeRunner {
    script: Arc<Mutex<Script>>,
    token_runs: Arc<AtomicUsize>,
    version_runs: Arc<AtomicUsize>,
    invocations: Invocations,
    delay: Duration,
}

impl FakeRunner {
    fn new(tokens: Vec<Step>) -> Self {
        Self::with_versions(vec![Step::Version("2.90.0")], tokens)
    }

    fn with_versions(versions: Vec<Step>, tokens: Vec<Step>) -> Self {
        Self {
            script: Arc::new(Mutex::new(Script { versions, tokens })),
            ..Self::default()
        }
    }

    fn token_runs(&self) -> usize {
        self.token_runs.load(Ordering::SeqCst)
    }
}

fn output(code: i32, stdout: String, stderr: &str) -> Output {
    Output {
        code: Some(code),
        stdout: Zeroizing::new(stdout.into_bytes()),
        stderr: Zeroizing::new(stderr.as_bytes().to_vec()),
    }
}

#[async_trait]
impl CommandRunner for FakeRunner {
    async fn run(&self, invocation: Invocation<'_>) -> Result<Output, RunError> {
        self.invocations.lock().unwrap().push((
            invocation.args.iter().map(|arg| arg.to_string()).collect(),
            invocation.env.remove.to_vec(),
        ));
        let version = invocation.args.first() == Some(&"version");
        let step = {
            let mut script = self.script.lock().unwrap();
            let queue = if version {
                self.version_runs.fetch_add(1, Ordering::SeqCst);
                &mut script.versions
            } else {
                self.token_runs.fetch_add(1, Ordering::SeqCst);
                &mut script.tokens
            };
            queue.remove(0)
        };
        tokio::time::sleep(self.delay).await;
        match step {
            Step::Token { secret, expires_on } => Ok(output(
                0,
                format!(
                    r#"{{"accessToken":"{secret}","expiresOn":"2026-01-01 00:00:00.000000","expires_on":{expires_on},"tenant":"{TENANT}","tokenType":"Bearer"}}"#
                ),
                "",
            )),
            Step::Version(version) => Ok(output(
                0,
                format!(r#"{{"azure-cli":"{version}","extensions":{{}}}}"#),
                "",
            )),
            Step::Fail { code, stderr } => Ok(output(code, String::new(), stderr)),
            Step::Raw(stdout) => Ok(output(0, stdout.into(), "")),
            Step::Err(error) => Err(error),
        }
    }
}

fn token(secret: &'static str, expires_on: i64) -> Step {
    Step::Token { secret, expires_on }
}

fn source(runner: FakeRunner, now: Arc<AtomicI64>) -> AzCliTokenSource<FakeRunner> {
    AzCliTokenSource::new(
        Some(PathBuf::from("/fixture/az")),
        runner,
        Arc::new(move || now.load(Ordering::SeqCst)),
    )
}

fn at(seconds: i64) -> Arc<AtomicI64> {
    Arc::new(AtomicI64::new(seconds))
}

fn tenant() -> TenantId {
    TenantId::parse(TENANT).unwrap()
}

#[tokio::test]
async fn requests_the_azure_devops_resource_for_the_tenant_without_agent_session() {
    let runner = FakeRunner::new(vec![token("fixture.token", 5_000)]);
    let source = source(runner.clone(), at(1_000));
    let token = source.token(&tenant()).await.unwrap();
    assert_eq!((token.secret(), token.expires_on), ("fixture.token", 5_000));
    assert!(!format!("{token:?}").contains("fixture.token"));
    let invocations = runner.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[0].0, ["version", "--output", "json"]);
    assert_eq!(
        invocations[1].0,
        [
            "account",
            "get-access-token",
            "--resource",
            "499b84ac-1321-427f-aa17-267ca6975798",
            "--tenant",
            TENANT,
            "--output",
            "json"
        ]
    );
    for (args, removed) in invocations.iter() {
        assert!(removed.contains(&"COPILOT_AGENT_SESSION_ID"));
        // Passable to `az.cmd` on Windows.
        assert!(args.iter().all(|arg| crate::cli_process::batch_safe(arg)));
    }
}

#[tokio::test]
async fn caches_within_lifetime_and_refreshes_five_minutes_before_expiry() {
    let now = at(1_000);
    let runner = FakeRunner::new(vec![token("first", 5_000), token("second", 9_000)]);
    let source = source(runner.clone(), now.clone());
    source.token(&tenant()).await.unwrap();
    now.store(5_000 - REFRESH_MARGIN_SECONDS - 1, Ordering::SeqCst);
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "first");
    assert_eq!(runner.token_runs(), 1);
    now.store(5_000 - REFRESH_MARGIN_SECONDS, Ordering::SeqCst);
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "second");
    assert_eq!(runner.token_runs(), 2);
    assert_eq!(runner.version_runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn eight_concurrent_callers_share_one_azure_cli_run() {
    let mut runner = FakeRunner::new(vec![token("shared", 5_000)]);
    runner.delay = Duration::from_millis(50);
    let source = Arc::new(source(runner.clone(), at(1_000)));
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let source = source.clone();
            tokio::spawn(async move { source.token(&tenant()).await.map(|t| t.expires_on) })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.unwrap(), Ok(5_000));
    }
    assert_eq!(runner.token_runs(), 1);
}

#[tokio::test]
async fn tenants_are_cached_independently() {
    let runner = FakeRunner::new(vec![token("one", 5_000), token("two", 5_000)]);
    let source = source(runner.clone(), at(1_000));
    let other = TenantId::parse("00000000-0000-0000-0000-000000000002").unwrap();
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "one");
    assert_eq!(source.token(&other).await.unwrap().secret(), "two");
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "one");
    assert_eq!(runner.token_runs(), 2);
}

#[tokio::test]
async fn unauthorized_invalidates_and_retries_once() {
    let runner = FakeRunner::new(vec![token("rejected", 5_000), token("accepted", 6_000)]);
    let source = source(runner.clone(), at(1_000));
    let seen = Mutex::new(Vec::new());
    let result: Result<&str, AuthorizedError<()>> = with_token(&source, &tenant(), |token| {
        seen.lock().unwrap().push(token.secret().to_owned());
        async move {
            if token.secret() == "rejected" {
                Err(Rejection::Unauthorized)
            } else {
                Ok("read")
            }
        }
    })
    .await;
    assert_eq!(result, Ok("read"));
    assert_eq!(*seen.lock().unwrap(), ["rejected", "accepted"]);
    assert_eq!(runner.token_runs(), 2);
}

#[tokio::test]
async fn persistent_unauthorized_stops_after_one_retry() {
    let runner = FakeRunner::new(vec![token("a", 5_000), token("b", 5_000)]);
    let source = source(runner.clone(), at(1_000));
    let calls = AtomicUsize::new(0);
    let result: Result<(), AuthorizedError<()>> = with_token(&source, &tenant(), |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err(Rejection::Unauthorized) }
    })
    .await;
    assert_eq!(result, Err(AuthorizedError::Unauthorized));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(runner.token_runs(), 2);

    let failed: Result<(), AuthorizedError<u16>> = with_token(&source, &tenant(), |_| async {
        Err(Rejection::Failed(500))
    })
    .await;
    assert_eq!(failed, Err(AuthorizedError::Failed(500)));
    assert_eq!(runner.token_runs(), 2);
}

#[tokio::test]
async fn concurrent_unauthorized_callers_trigger_one_refresh() {
    let mut runner = FakeRunner::new(vec![token("rejected", 5_000), token("accepted", 6_000)]);
    runner.delay = Duration::from_millis(20);
    let source = Arc::new(source(runner.clone(), at(1_000)));
    let rejected = source.token(&tenant()).await.unwrap();
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let (source, rejected) = (source.clone(), rejected.clone());
            tokio::spawn(async move {
                source.invalidate(&tenant(), &rejected);
                source.token(&tenant()).await.map(|t| t.expires_on)
            })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.unwrap(), Ok(6_000));
    }
    assert_eq!(runner.token_runs(), 2);
}

#[tokio::test]
async fn failures_are_not_cached() {
    let runner = FakeRunner::new(vec![Step::Err(RunError::Timeout), token("later", 5_000)]);
    let source = source(runner, at(1_000));
    assert_eq!(
        source.token(&tenant()).await.unwrap_err(),
        TokenError::Timeout
    );
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "later");
}

#[tokio::test]
async fn rejects_azure_cli_older_than_the_minimum_until_upgraded() {
    let runner = FakeRunner::with_versions(
        vec![Step::Version("2.53.1"), Step::Version("2.54.0")],
        vec![token("upgraded", 5_000)],
    );
    let source = source(runner.clone(), at(1_000));
    assert_eq!(
        source.token(&tenant()).await.unwrap_err(),
        TokenError::AzTooOld
    );
    assert_eq!(runner.token_runs(), 0);
    assert_eq!(source.token(&tenant()).await.unwrap().secret(), "upgraded");
}

#[tokio::test]
async fn maps_process_failures_to_typed_errors() {
    let missing = AzCliTokenSource::new(None, FakeRunner::default(), Arc::new(|| 0));
    assert_eq!(
        missing.token(&tenant()).await.unwrap_err(),
        TokenError::AzMissing
    );
    let cases = [
        (Step::Err(RunError::NotFound), TokenError::AzMissing),
        (Step::Err(RunError::Timeout), TokenError::Timeout),
        (
            Step::Err(RunError::OutputLimit),
            TokenError::MalformedOutput,
        ),
        (
            Step::Err(RunError::Failed),
            TokenError::Other {
                exit_code: None,
                aadsts: None,
            },
        ),
        (
            Step::Fail {
                code: 1,
                stderr: "ERROR: Please run 'az login' to setup account.\n",
            },
            TokenError::NotLoggedIn,
        ),
        (
            Step::Fail {
                code: 1,
                stderr: "ERROR: AADSTS500213: blocked",
            },
            TokenError::TenantAccessBlocked,
        ),
        (Step::Raw("not json"), TokenError::MalformedOutput),
    ];
    for (step, expected) in cases {
        let source = source(FakeRunner::new(vec![step]), at(0));
        assert_eq!(source.token(&tenant()).await.unwrap_err(), expected);
    }
    let broken = source(
        FakeRunner::with_versions(vec![Step::Raw("{}")], vec![]),
        at(0),
    );
    assert_eq!(
        broken.token(&tenant()).await.unwrap_err(),
        TokenError::MalformedOutput
    );
}
