use serde::Serialize;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
#[path = "startup/macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::LoginRegistration;
#[cfg(windows)]
#[path = "startup/windows.rs"]
mod windows;
#[cfg(windows)]
pub use windows::LoginRegistration;

#[cfg(test)]
#[path = "startup/retention_tests.rs"]
mod retention_tests;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationStatus {
    Absent,
    Registered,
    Invalid,
}

impl LoginRegistration {
    pub fn for_app(home: PathBuf, executable: PathBuf) -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::new(
                home.join("Library/LaunchAgents/PR Sniper.plist"),
                executable,
            )
        }
        #[cfg(windows)]
        {
            let _ = home;
            Self::new(
                r"Software\Microsoft\Windows\CurrentVersion\Run".into(),
                "com.jdylanmc.pr-sniper".into(),
                executable,
            )
        }
    }
}
