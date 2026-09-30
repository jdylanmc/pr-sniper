use pr_sniper_lib::github::{
    credentials::{
        CredentialSource, GhCredentialSource, ProcessOutput, ProcessRunner, SystemRunner,
    },
    ConnectionError,
};
use std::{cell::RefCell, path::Path};

struct FakeCli {
    arguments: RefCell<Vec<Vec<String>>>,
}

#[test]
fn missing_executable_is_not_a_signed_in_account() {
    let source = GhCredentialSource::new("/nonexistent/pr-sniper-fixture/gh".into(), SystemRunner);
    assert_eq!(source.acquire().unwrap_err(), ConnectionError::MissingCli);
}

impl ProcessRunner for FakeCli {
    fn run(&self, _: &Path, args: &[&str]) -> Result<ProcessOutput, ConnectionError> {
        self.arguments
            .borrow_mut()
            .push(args.iter().map(|arg| arg.to_string()).collect());
        Ok(ProcessOutput {
            code: Some(0),
            stdout: if args == ["--version"] {
                b"gh version 2.101.0 (2026-09-15)\nhttps://github.com/cli/cli/releases/tag/v2.101.0\n".to_vec()
            } else {
                b"gho_fixture_secret_not_a_real_token\n".to_vec()
            },
        })
    }
}

#[test]
fn credentials_are_obtained_without_secret_arguments_or_debug_output() {
    let cli = FakeCli {
        arguments: RefCell::new(Vec::new()),
    };
    let source = GhCredentialSource::new("/trusted/gh".into(), &cli);
    let credential = source.acquire().expect("healthy signed-in CLI");
    assert_eq!(format!("{credential:?}"), "Credential([REDACTED])");
    assert_eq!(
        *cli.arguments.borrow(),
        vec![
            vec!["--version"],
            vec!["auth", "token", "--hostname", "github.com"]
        ]
    );
}

impl ProcessRunner for &FakeCli {
    fn run(&self, path: &Path, args: &[&str]) -> Result<ProcessOutput, ConnectionError> {
        (*self).run(path, args)
    }
}

struct CliFixture(std::path::PathBuf);

impl CliFixture {
    fn new(scenario: &str) -> Self {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".pr-sniper-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        let output = std::process::Command::new("rustc")
            .arg("--edition=2021")
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/support/credential_cli.rs"),
            )
            .arg("-o")
            .arg(fixture.executable())
            .output()
            .expect("compile native test-only credential CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::write(fixture.0.join("scenario"), scenario).unwrap();
        fixture
    }

    fn executable(&self) -> std::path::PathBuf {
        self.0.join(format!("gh{}", std::env::consts::EXE_SUFFIX))
    }

    fn source(&self) -> GhCredentialSource<SystemRunner> {
        GhCredentialSource::new(self.executable(), SystemRunner)
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn real_cli_shim_cannot_pass_by_merely_existing() {
    let cli = CliFixture::new("shim");
    assert_eq!(
        cli.source().acquire().unwrap_err(),
        ConnectionError::BrokenCli
    );
}

#[test]
fn real_signed_out_cli_does_not_echo_secret_like_stderr() {
    let cli = CliFixture::new("signed-out");
    let error = cli.source().acquire().unwrap_err();
    assert_eq!(error, ConnectionError::SignedOut);
    assert!(!format!("{error:?}").contains("secret"));
}

#[test]
fn real_cli_success_returns_only_an_opaque_credential() {
    let cli = CliFixture::new("success");
    let credential = cli.source().acquire().unwrap();
    assert_eq!(format!("{credential:?}"), "Credential([REDACTED])");
}

#[test]
fn real_broken_cli_exit_is_distinct_from_signed_out() {
    let cli = CliFixture::new("broken");
    assert_eq!(
        cli.source().acquire().unwrap_err(),
        ConnectionError::BrokenCli
    );
}

#[test]
fn real_cli_capture_is_bounded() {
    let cli = CliFixture::new("overflow");
    assert_eq!(
        cli.source().acquire().unwrap_err(),
        ConnectionError::BrokenCli
    );
}

#[test]
fn stalled_cli_is_bounded_and_terminated() {
    let cli = CliFixture::new("stalled");
    let start = std::time::Instant::now();
    assert_eq!(
        cli.source().acquire().unwrap_err(),
        ConnectionError::Timeout
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(15));
}
