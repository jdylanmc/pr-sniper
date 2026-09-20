use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub launch_at_login: bool,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        // Paired-TDD bootstrap: persistence is intentionally not implemented yet.
        let _ = &self.root;
        Ok(Settings::default())
    }

    pub fn save_settings(&self, _settings: &Settings) -> Result<(), String> {
        Ok(())
    }
}
