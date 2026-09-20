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
