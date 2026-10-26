use super::ConnectionError;
use crate::cli_process::{self, CommandRunner, EnvPolicy, Invocation, Limits, RunError};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;

pub struct Credential(pub(super) Zeroizing<String>);

impl Credential {
    pub(super) fn from_secret(secret: impl Into<String>) -> Self {
        Self(Zeroizing::new(secret.into()))
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credential([REDACTED])")
    }
}

pub trait CredentialSource {
    fn acquire(&self) -> Result<Credential, ConnectionError>;
}

pub struct ProcessOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
}

pub trait ProcessRunner {
    fn run(&self, executable: &Path, args: &[&str]) -> Result<ProcessOutput, ConnectionError>;
}

pub struct SystemRunner;

const GH_ENV: EnvPolicy = EnvPolicy {
    remove: &["GH_DEBUG"],
    set: &[("GH_PROMPT_DISABLED", "1")],
};

impl ProcessRunner for SystemRunner {
    fn run(&self, executable: &Path, args: &[&str]) -> Result<ProcessOutput, ConnectionError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ConnectionError::BrokenCli)?;
        let mut output = runtime
            .block_on(cli_process::SystemRunner.run(Invocation {
                program: executable,
                args,
                env: &GH_ENV,
                limits: Limits {
                    timeout: Duration::from_secs(10),
                    output_bytes: 16_384,
                },
            }))
            .map_err(|error| match error {
                RunError::NotFound => ConnectionError::MissingCli,
                RunError::Timeout => ConnectionError::Timeout,
                RunError::OutputLimit | RunError::Failed => ConnectionError::BrokenCli,
            })?;
        Ok(ProcessOutput {
            code: output.code,
            stdout: std::mem::take(&mut *output.stdout),
        })
    }
}

pub struct GhCredentialSource<R> {
    executable: PathBuf,
    runner: R,
}

impl<R: ProcessRunner> GhCredentialSource<R> {
    pub fn new(executable: PathBuf, runner: R) -> Self {
        Self { executable, runner }
    }
}

impl GhCredentialSource<SystemRunner> {
    pub fn discover() -> Result<Self, ConnectionError> {
        let paths = std::env::var_os("PATH").unwrap_or_default();
        crate::process_path::executable("gh", &paths)
            .map(|executable| Self::new(executable, SystemRunner))
            .ok_or(ConnectionError::MissingCli)
    }
}

impl<R: ProcessRunner> CredentialSource for GhCredentialSource<R> {
    fn acquire(&self) -> Result<Credential, ConnectionError> {
        let version = self.runner.run(&self.executable, &["--version"])?;
        let version_text =
            std::str::from_utf8(&version.stdout).map_err(|_| ConnectionError::BrokenCli)?;
        if version.code != Some(0)
            || !version_text.starts_with("gh version ")
            || !version_text.contains("https://github.com/cli/cli/")
        {
            return Err(ConnectionError::BrokenCli);
        }
        let mut output = self.runner.run(
            &self.executable,
            &["auth", "token", "--hostname", "github.com"],
        )?;
        if output.code != Some(0) {
            return Err(if output.code == Some(1) {
                ConnectionError::SignedOut
            } else {
                ConnectionError::BrokenCli
            });
        }
        let bytes = Zeroizing::new(std::mem::take(&mut output.stdout));
        let token = std::str::from_utf8(&bytes)
            .map_err(|_| ConnectionError::BrokenCli)?
            .trim();
        if token.is_empty() {
            return Err(ConnectionError::SignedOut);
        }
        if token.len() > 16_384 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ConnectionError::BrokenCli);
        }
        Ok(Credential(Zeroizing::new(token.into())))
    }
}
