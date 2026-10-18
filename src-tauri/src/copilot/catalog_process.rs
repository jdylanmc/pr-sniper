use github_copilot_sdk::{CliProgram, Client, ClientMode, ClientOptions, Transport};
use std::{io, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

// Catalog lookups never create sessions. Keep their stdio child outside the
// SDK's startup future so a cancelled/failed handshake can kill AND reap it.
pub(super) struct CatalogProcess {
    child: Child,
    directory: std::path::PathBuf,
}

impl CatalogProcess {
    pub(super) fn spawn(options: &ClientOptions) -> io::Result<Self> {
        let CliProgram::Path(program) = &options.program else {
            return Err(io::Error::other("Catalog requires an explicit runtime."));
        };
        if !matches!(options.transport, Transport::Stdio)
            || options.mode != ClientMode::Empty
            || options.use_logged_in_user != Some(false)
        {
            return Err(io::Error::other("Catalog requires isolated stdio options."));
        }
        let mut command = Command::new(program);
        command
            .kill_on_drop(true)
            .args(&options.prefix_args)
            .args([
                "--server",
                "--stdio",
                "--no-auto-update",
                "--log-level",
                "none",
                "--no-auto-login",
            ])
            .env("COPILOT_DISABLE_KEYTAR", "1");
        if let Some(token) = &options.github_token {
            command
                .args(["--auth-token-env", "COPILOT_SDK_AUTH_TOKEN"])
                .env("COPILOT_SDK_AUTH_TOKEN", token);
        }
        if let Some(directory) = &options.base_directory {
            command.env("COPILOT_HOME", directory);
        }
        command.envs(options.env.iter().cloned());
        for key in &options.env_remove {
            command.env_remove(key);
        }
        let child = command
            .current_dir(&options.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            child,
            directory: options.working_directory.clone(),
        })
    }

    pub(super) async fn connect(&mut self) -> github_copilot_sdk::Result<Client> {
        let client = Client::from_streams(
            self.child.stdout.take().expect("catalog stdout is piped"),
            self.child.stdin.take().expect("catalog stdin is piped"),
            self.directory.clone(),
        )?;
        client.verify_protocol_version().await?;
        Ok(client)
    }

    pub(super) async fn shutdown(&mut self, client: &Client) -> io::Result<()> {
        let _ =
            tokio::time::timeout(Duration::from_secs(5), client.rpc().runtime().shutdown()).await;
        client.force_stop();
        self.terminate().await
    }

    pub(super) async fn terminate(&mut self) -> io::Result<()> {
        self.child.kill().await
    }

    #[cfg(test)]
    pub(super) fn pid(&self) -> u32 {
        self.child.id().expect("owned catalog child is running")
    }
}
