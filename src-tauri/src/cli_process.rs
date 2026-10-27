//! Bounded provider CLI processes: absolute program, closed stdin, per-tool
//! environment, size-limited output and a timeout that kills only our child.
//! Output is never logged; stderr is for the caller's classifier only.

use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use zeroize::{Zeroize, Zeroizing};

/// Environment changes applied to the child only; the parent is untouched.
#[derive(Debug, Clone, Copy)]
pub struct EnvPolicy {
    pub remove: &'static [&'static str],
    pub set: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    /// Per stream; a larger stream fails the run.
    pub output_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Invocation<'a> {
    pub program: &'a Path,
    pub args: &'a [&'a str],
    pub env: &'a EnvPolicy,
    pub limits: Limits,
}

pub struct Output {
    pub code: Option<i32>,
    pub stdout: Zeroizing<Vec<u8>>,
    pub stderr: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("code", &self.code)
            .field("stdout", &"[REDACTED]")
            .field("stderr", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    NotFound,
    Timeout,
    OutputLimit,
    Failed,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, invocation: Invocation<'_>) -> Result<Output, RunError>;
}

#[async_trait]
impl<R: CommandRunner + ?Sized> CommandRunner for std::sync::Arc<R> {
    async fn run(&self, invocation: Invocation<'_>) -> Result<Output, RunError> {
        (**self).run(invocation).await
    }
}

pub struct SystemRunner;

fn command(invocation: &Invocation<'_>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(invocation.program);
    command.args(invocation.args);
    for name in invocation.env.remove {
        command.env_remove(name);
    }
    for (name, value) in invocation.env.set {
        command.env(name, value);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

#[async_trait]
impl CommandRunner for SystemRunner {
    async fn run(&self, invocation: Invocation<'_>) -> Result<Output, RunError> {
        let mut child = command(&invocation)
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => RunError::NotFound,
                _ => RunError::Failed,
            })?;
        let stdout = child.stdout.take().ok_or(RunError::Failed)?;
        let stderr = child.stderr.take().ok_or(RunError::Failed)?;
        let limit = invocation.limits.output_bytes;
        let result = tokio::time::timeout(invocation.limits.timeout, async {
            tokio::try_join!(
                async { child.wait().await.map_err(|_| RunError::Failed) },
                bounded(stdout, limit),
                bounded(stderr, limit)
            )
        })
        .await;
        match result {
            Ok(Ok((status, stdout, stderr))) => Ok(Output {
                code: status.code(),
                stdout,
                stderr,
            }),
            failure => {
                // Reap our exact child; never kill processes by name.
                let _ = child.kill().await;
                Err(match failure {
                    Err(_) => RunError::Timeout,
                    Ok(Err(error)) => error,
                    Ok(Ok(_)) => RunError::Failed,
                })
            }
        }
    }
}

async fn bounded(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, RunError> {
    let mut bytes = Vec::new();
    let read = reader.take(limit as u64 + 1).read_to_end(&mut bytes).await;
    if read.is_err() || bytes.len() > limit {
        bytes.zeroize();
        return Err(if read.is_err() {
            RunError::Failed
        } else {
            RunError::OutputLimit
        });
    }
    Ok(Zeroizing::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    const POLICY: EnvPolicy = EnvPolicy {
        remove: &["PR_SNIPER_FIXTURE_REMOVED"],
        set: &[("PR_SNIPER_FIXTURE_SET", "yes")],
    };

    fn limits(timeout_ms: u64, output_bytes: usize) -> Limits {
        Limits {
            timeout: Duration::from_millis(timeout_ms),
            output_bytes,
        }
    }

    #[test]
    fn applies_the_env_policy_to_the_child_only() {
        let invocation = Invocation {
            program: Path::new("/fixture/tool"),
            args: &["a"],
            env: &POLICY,
            limits: limits(1_000, 16),
        };
        let command = command(&invocation);
        let envs: Vec<_> = command.as_std().get_envs().collect();
        assert!(envs.contains(&(OsStr::new("PR_SNIPER_FIXTURE_REMOVED"), None)));
        assert!(envs.contains(&(OsStr::new("PR_SNIPER_FIXTURE_SET"), Some(OsStr::new("yes")))));
    }

    #[cfg(unix)]
    fn sh(script: &str, limits: Limits) -> Result<Output, RunError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(SystemRunner.run(Invocation {
            program: Path::new("/bin/sh"),
            args: &["-c", script],
            env: &POLICY,
            limits,
        }))
    }

    #[cfg(unix)]
    #[test]
    fn runs_with_closed_stdin_and_separate_streams() {
        let output = sh(
            "cat; printf out; printf err >&2; printf \"$PR_SNIPER_FIXTURE_SET\" >&2; exit 3",
            limits(5_000, 64),
        )
        .unwrap();
        assert_eq!(output.code, Some(3));
        assert_eq!(output.stdout.as_slice(), b"out");
        assert_eq!(output.stderr.as_slice(), b"erryes");
        assert!(!format!("{output:?}").contains("erryes"));
    }

    #[cfg(unix)]
    #[test]
    fn kills_its_child_on_timeout_and_rejects_oversized_output() {
        let started = std::time::Instant::now();
        assert_eq!(
            sh("exec sleep 5", limits(100, 64)).unwrap_err(),
            RunError::Timeout
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(
            sh("head -c 100 /dev/zero", limits(5_000, 64)).unwrap_err(),
            RunError::OutputLimit
        );
        assert_eq!(
            sh("head -c 64 /dev/zero", limits(5_000, 64))
                .unwrap()
                .stdout
                .len(),
            64
        );
    }

    #[test]
    fn missing_program_is_not_found() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(SystemRunner.run(Invocation {
            program: Path::new("/nonexistent/pr-sniper-fixture/tool"),
            args: &[],
            env: &POLICY,
            limits: limits(1_000, 16),
        }));
        assert_eq!(result.unwrap_err(), RunError::NotFound);
    }
}
