use super::RegistrationStatus;
use crate::storage::Store;
use std::{fs, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    },
};

pub struct LoginRegistration {
    key: String,
    name: String,
    executable: PathBuf,
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

struct Value {
    kind: u32,
    bytes: Vec<u8>,
}

fn wide(value: &str) -> Result<Vec<u16>, String> {
    if value.contains('\0') {
        return Err("Invalid launch-at-login registration.".into());
    }
    Ok(value.encode_utf16().chain([0]).collect())
}

impl LoginRegistration {
    // The application owns one named value, never the containing Run key.
    // Explicit locations also let native tests own an entirely separate key.
    pub fn new(key: String, name: String, executable: PathBuf) -> Self {
        Self {
            key,
            name,
            executable,
        }
    }

    fn open(&self, write: bool, create: bool) -> Result<Option<Key>, String> {
        let name = wide(&self.key)?;
        let access = if write {
            KEY_SET_VALUE
        } else {
            KEY_QUERY_VALUE
        };
        let mut key = ptr::null_mut();
        let status = unsafe {
            if create {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    name.as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    access,
                    ptr::null(),
                    &mut key,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, access, &mut key)
            }
        };
        match status {
            ERROR_SUCCESS => Ok(Some(Key(key))),
            ERROR_FILE_NOT_FOUND if !create => Ok(None),
            _ => Err("Cannot access the launch-at-login registration.".into()),
        }
    }

    fn read(&self) -> Result<Option<Value>, String> {
        let Some(key) = self.open(false, false)? else {
            return Ok(None);
        };
        let name = wide(&self.name)?;
        let mut kind = 0;
        let mut size = 0;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS || size > 65_536 {
            return Err("Cannot read the launch-at-login registration.".into());
        }
        let mut bytes = vec![0; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return Err("Cannot read the launch-at-login registration.".into());
        }
        bytes.truncate(size as usize);
        Ok(Some(Value { kind, bytes }))
    }

    fn command(&self) -> Result<Value, String> {
        let executable = self
            .executable
            .to_str()
            .ok_or("The application path is not valid Unicode.")?;
        if !self.executable.is_absolute() || executable.contains('"') || executable.contains('\0') {
            return Err("Launch at login requires an absolute application executable path.".into());
        }
        let command = format!("\"{executable}\"");
        // Windows documents a 260-character limit for Run value command lines.
        if command.encode_utf16().count() > 260 {
            return Err("The application path is too long for launch at login.".into());
        }
        Ok(Value {
            kind: REG_SZ,
            bytes: wide(&command)?
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
        })
    }

    fn executable_exists(&self) -> Result<bool, String> {
        match fs::metadata(&self.executable) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err("Cannot inspect the registered application executable.".into()),
        }
    }

    pub fn status(&self) -> Result<RegistrationStatus, String> {
        let Some(value) = self.read()? else {
            return Ok(RegistrationStatus::Absent);
        };
        let Ok(expected) = self.command() else {
            return Ok(RegistrationStatus::Invalid);
        };
        if !self.executable_exists()? {
            return Ok(RegistrationStatus::Invalid);
        }
        Ok(
            if value.kind == expected.kind && value.bytes == expected.bytes {
                // Presence is not proof of launch: Startup Apps can disable this entry.
                RegistrationStatus::Registered
            } else {
                RegistrationStatus::Invalid
            },
        )
    }

    fn replace(&self, value: Option<&Value>) -> Result<(), String> {
        let Some(key) = self.open(true, value.is_some())? else {
            return Ok(());
        };
        let name = wide(&self.name)?;
        let status = unsafe {
            match value {
                Some(value) => RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    value.kind,
                    value.bytes.as_ptr(),
                    value.bytes.len() as u32,
                ),
                None => RegDeleteValueW(key.0, name.as_ptr()),
            }
        };
        if status == ERROR_SUCCESS || (value.is_none() && status == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err("Cannot update the launch-at-login registration.".into())
        }
    }

    pub fn set_enabled(&self, store: &Store, enabled: bool) -> Result<(), String> {
        let mut settings = store.load_settings()?;
        let previous = self.read()?;
        let next = if enabled {
            if !self.executable_exists()? {
                return Err("Launch at login requires an existing application executable.".into());
            }
            Some(self.command()?)
        } else {
            None
        };
        self.replace(next.as_ref())?;
        settings.launch_at_login = enabled;
        if let Err(error) = store.save_settings(&settings) {
            if self.replace(previous.as_ref()).is_err() {
                return Err("Settings were not saved and the previous login registration could not be restored. Inspect Windows Startup Apps.".into());
            }
            return Err(error);
        }
        Ok(())
    }
}
