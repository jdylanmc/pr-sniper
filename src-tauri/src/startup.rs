use crate::storage::{Settings, Store};
use plist::{Dictionary, Value};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

const LABEL: &str = "PR Sniper";

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationStatus {
    Absent,
    Registered,
    Invalid,
}

pub struct LoginRegistration {
    path: PathBuf,
    executable: PathBuf,
}

impl LoginRegistration {
    pub fn new(path: PathBuf, executable: PathBuf) -> Self {
        Self { path, executable }
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err("Cannot read the launch-at-login registration.".into()),
        }
    }

    pub fn status(&self) -> Result<RegistrationStatus, String> {
        let Some(bytes) = self.read()? else {
            return Ok(RegistrationStatus::Absent);
        };
        let Ok(Value::Dictionary(registration)) = Value::from_reader(std::io::Cursor::new(bytes))
        else {
            return Ok(RegistrationStatus::Invalid);
        };
        let expected_arguments = vec![Value::String(
            self.executable.to_string_lossy().into_owned(),
        )];
        let valid = registration.get("Label").and_then(Value::as_string) == Some(LABEL)
            && registration.get("RunAtLoad").and_then(Value::as_boolean) == Some(true)
            && registration
                .get("ProgramArguments")
                .and_then(Value::as_array)
                == Some(&expected_arguments)
            && registration.len() == 3;
        if !valid {
            return Ok(RegistrationStatus::Invalid);
        }
        match fs::metadata(&self.executable) {
            Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
                Ok(RegistrationStatus::Registered)
            }
            Ok(_) => Ok(RegistrationStatus::Invalid),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(RegistrationStatus::Invalid),
            Err(_) => Err("Cannot inspect the registered application executable.".into()),
        }
    }

    fn replace(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        let Some(bytes) = bytes else {
            return match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(_) => Err("Cannot remove the launch-at-login registration.".into()),
            };
        };
        let directory = self.path.parent().ok_or("Invalid registration location.")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|_| "Cannot create the launch-at-login directory.")?;
        let temporary = directory.join(format!(".pr-sniper-{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "Cannot stage the launch-at-login registration.")?;
        let result = file
            .write_all(bytes)
            .and_then(|_| file.sync_all())
            .and_then(|_| fs::rename(&temporary, &self.path));
        if result.is_err() {
            return match fs::remove_file(temporary) {
                Ok(()) => Err("Cannot replace the launch-at-login registration.".into()),
                Err(_) => Err(
                    "Cannot replace or clean up the staged launch-at-login registration.".into(),
                ),
            };
        }
        Ok(())
    }

    pub fn set_enabled(&self, store: &Store, enabled: bool) -> Result<(), String> {
        store.load_settings()?;
        let previous = self.read()?;
        let registration = if enabled {
            let metadata = fs::metadata(&self.executable)
                .map_err(|_| "Cannot inspect the application executable.")?;
            if !self.executable.is_absolute()
                || !metadata.is_file()
                || metadata.permissions().mode() & 0o111 == 0
            {
                return Err(
                    "Launch at login requires an absolute, executable application path.".into(),
                );
            }
            let mut values = Dictionary::new();
            values.insert("Label".into(), Value::String(LABEL.into()));
            values.insert("RunAtLoad".into(), Value::Boolean(true));
            values.insert(
                "ProgramArguments".into(),
                Value::Array(vec![Value::String(
                    self.executable
                        .to_str()
                        .ok_or("The application path is not valid Unicode.")?
                        .into(),
                )]),
            );
            let mut bytes = Vec::new();
            Value::Dictionary(values)
                .to_writer_xml(&mut bytes)
                .map_err(|_| "Cannot encode the launch-at-login registration.")?;
            Some(bytes)
        } else {
            None
        };
        self.replace(registration.as_deref())?;
        if let Err(error) = store.save_settings(&Settings {
            launch_at_login: enabled,
        }) {
            if self.replace(previous.as_deref()).is_err() {
                return Err("Settings were not saved and the previous login registration could not be restored. Inspect macOS Login Items.".into());
            }
            return Err(error);
        }
        Ok(())
    }
}
