use serde::Serialize;
use std::path::{Path, PathBuf};

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
#[path = "startup/repair_tests.rs"]
mod repair_tests;
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

impl LoginRegistration {
    /// Launch repair for a stale or broken registration while the saved
    /// preference is On. Only an installed app re-points it; a dev or
    /// temporary build leaves it for the explicit Settings Repair action.
    pub fn repair_on_launch(&self, enabled: bool, installed: bool) -> Result<bool, String> {
        if !enabled || !installed || self.status()? != RegistrationStatus::Invalid {
            return Ok(false);
        }
        self.register()?;
        Ok(true)
    }
}

/// Install locations whose executable may rewrite host registrations at launch.
pub fn install_roots(home: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![PathBuf::from("/Applications"), home.join("Applications")]
    }
    #[cfg(windows)]
    {
        let _ = home;
        std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("PR Sniper"))
            .into_iter()
            .collect()
    }
}

/// macOS: `<root>/<name>.app/Contents/MacOS/<exe>`; Windows: `<root>\<exe>`.
pub fn installed_under(executable: &Path, roots: &[PathBuf]) -> bool {
    #[cfg(target_os = "macos")]
    let parent = {
        let macos = executable.parent();
        let contents = macos.and_then(Path::parent);
        let bundle = contents.and_then(Path::parent);
        if macos.and_then(Path::file_name) != Some("MacOS".as_ref())
            || contents.and_then(Path::file_name) != Some("Contents".as_ref())
            || bundle.and_then(Path::extension) != Some("app".as_ref())
        {
            return false;
        }
        bundle.and_then(Path::parent)
    };
    #[cfg(windows)]
    let parent = executable.parent();
    let Some(parent) = parent else {
        return false;
    };
    let key = |path: &Path| {
        let text = path
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    executable.is_absolute() && roots.iter().any(|root| key(root) == key(parent))
}

pub fn installed_executable(executable: &Path, home: &Path) -> bool {
    installed_under(executable, &install_roots(home))
}
