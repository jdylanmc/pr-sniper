use super::ConnectionError;
use std::path::{Path, PathBuf};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};
use zeroize::{Zeroize, Zeroizing};

pub struct Credential(pub(super) Zeroizing<String>);

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

impl ProcessRunner for SystemRunner {
    fn run(&self, executable: &Path, args: &[&str]) -> Result<ProcessOutput, ConnectionError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ConnectionError::BrokenCli)?;
        runtime.block_on(async {
            let mut child = tokio::process::Command::new(executable)
                .args(args)
                .env("GH_PROMPT_DISABLED", "1")
                .env_remove("GH_DEBUG")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::NotFound => ConnectionError::MissingCli,
                    _ => ConnectionError::BrokenCli,
                })?;
            let stdout = child.stdout.take().ok_or(ConnectionError::BrokenCli)?;
            let stderr = child.stderr.take().ok_or(ConnectionError::BrokenCli)?;
            let result = tokio::time::timeout(Duration::from_secs(10), async {
                tokio::try_join!(
                    async { child.wait().await.map_err(|_| ConnectionError::BrokenCli) },
                    bounded_output(stdout),
                    bounded_output(stderr)
                )
            })
            .await;
            match result {
                Ok(Ok((status, stdout, mut stderr))) => {
                    stderr.zeroize();
                    Ok(ProcessOutput {
                        code: status.code(),
                        stdout,
                    })
                }
                failure => {
                    // Reap our exact child; do not kill processes by name.
                    let _ = child.kill().await;
                    match failure {
                        Err(_) => Err(ConnectionError::Timeout),
                        _ => Err(ConnectionError::BrokenCli),
                    }
                }
            }
        })
    }
}

async fn bounded_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, ConnectionError> {
    let mut bytes = Vec::new();
    reader
        .take(16_385)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ConnectionError::BrokenCli)?;
    if bytes.len() > 16_384 {
        bytes.zeroize();
        return Err(ConnectionError::BrokenCli);
    }
    Ok(bytes)
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
        let mut directories: Vec<_> = std::env::split_paths(&paths)
            .filter(|path| path.is_absolute())
            .collect();
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
        for directory in directories {
            let executable = directory.join("gh");
            if executable.is_file() {
                return Ok(Self::new(executable, SystemRunner));
            }
        }
        Err(ConnectionError::MissingCli)
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
