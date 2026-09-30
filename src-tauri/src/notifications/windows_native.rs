//! Inbox Windows APIs. Persistent registration is separate from process lifetime.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use windows::{
    core::{implement, Interface, Ref, BOOL, GUID, HSTRING, PCWSTR},
    Data::Xml::Dom::XmlDocument,
    Win32::{
        Foundation::{CLASS_E_NOAGGREGATION, ERROR_FILE_NOT_FOUND, E_INVALIDARG, E_POINTER},
        Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID},
        System::{
            Com::{
                CoCreateInstance, CoRegisterClassObject, CoRevokeClassObject, CoTaskMemFree,
                IClassFactory, IClassFactory_Impl, IPersistFile, StructuredStorage::*,
                CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE, STGM_READ,
            },
            Registry::*,
            WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        },
        UI::{
            Notifications::{
                INotificationActivationCallback, INotificationActivationCallback_Impl,
                NOTIFICATION_USER_INPUT_DATA,
            },
            Shell::{
                FOLDERID_LocalAppData, FOLDERID_Programs, IShellLinkW,
                PropertiesSystem::IPropertyStore, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
                SLGP_RAWPATH,
            },
        },
    },
    UI::Notifications::{NotificationSetting, ToastNotification, ToastNotificationManager},
};

pub const SERVER_ARG: &str = "--pr-sniper-notification-server";
pub const OPEN_ARG: &str = "--pr-sniper-notification-open";
pub const CLEANUP_ARG: &str = "--pr-sniper-notification-unregister";
const OWNER: &str = "PRSniperNotificationOwner";

pub type ActivationRequest = (Registration, String);

pub struct StartupQueue {
    ready: bool,
    stopped: bool,
    pending: Vec<ActivationRequest>,
}

impl StartupQueue {
    pub const fn new() -> Self {
        Self {
            ready: false,
            stopped: false,
            pending: Vec::new(),
        }
    }

    pub fn forward(
        &mut self,
        request: ActivationRequest,
    ) -> Result<Option<ActivationRequest>, String> {
        if self.stopped {
            return Err("PR Sniper is quitting; retry the notification after it exits.".into());
        }
        if self.ready {
            return Ok(Some(request));
        }
        if self.pending.len() == 16 {
            return Err(
                "Notification activation queue is full; retry after PR Sniper finishes starting."
                    .into(),
            );
        }
        self.pending.push(request);
        Ok(None)
    }

    pub fn ready(&mut self) -> Result<Vec<ActivationRequest>, String> {
        if self.stopped {
            return Err("Notification startup was cancelled because PR Sniper is quitting.".into());
        }
        self.ready = true;
        Ok(std::mem::take(&mut self.pending))
    }

    pub fn shutdown(&mut self) -> Vec<ActivationRequest> {
        self.stopped = true;
        std::mem::take(&mut self.pending)
    }
}

pub fn production_root() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_LocalAppData).map(|p| p.join("com.jdylanmc.pr-sniper"))
}

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }
        .map_err(|e| native_error("known folder lookup", e))?;
    let folder = unsafe { raw.to_string() };
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    folder
        .map(PathBuf::from)
        .map_err(|e| native_error("known folder path", e.into()))
}

pub fn native_error(stage: &str, error: windows::core::Error) -> String {
    format!(
        "Windows notification {stage} failed (0x{:08X}).",
        error.code().0 as u32
    )
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn shell_path(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("Notification path is not Unicode.")?;
    Ok(match value.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => value.strip_prefix(r"\\?\").unwrap_or(value).to_string(),
    })
}

pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|e| native_error("initialization", e))?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub version: u8,
    pub profile: String,
    pub root: PathBuf,
    pub executable: PathBuf,
    pub credential_service: Option<String>,
}

impl Registration {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || uuid::Uuid::parse_str(&self.profile)
                .map(|id| id.to_string() != self.profile)
                .unwrap_or(true)
            || [&self.root, &self.executable].iter().any(|p| {
                !p.is_absolute()
                    || p.to_str()
                        .is_none_or(|s| s.contains(['"', '|']) || s.chars().any(char::is_control))
            })
            || self.credential_service.as_ref().is_some_and(|s| {
                !s.starts_with("com.jdylanmc.pr-sniper.tests.")
                    || s.len() > 200
                    || s.bytes()
                        .any(|b| !(b.is_ascii_alphanumeric() || b"._-".contains(&b)))
            })
        {
            return Err("Invalid notification profile identity; no destination was opened.".into());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_vec(self)
            .map(|v| URL_SAFE_NO_PAD.encode(v))
            .map_err(|_| "Notification identity could not be encoded.".into())
    }

    pub fn decode(token: &str) -> Result<Self, String> {
        if token.len() > 16000 {
            return Err("Notification activation identity is too long.".into());
        }
        let value: Self = URL_SAFE_NO_PAD
            .decode(token)
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .ok_or("Notification activation identity is invalid.")?;
        value.validate()?;
        Ok(value)
    }

    pub fn aumid(&self) -> String {
        format!(
            "com.jdylanmc.pr-sniper.notify.{}",
            self.profile.replace('-', "")
        )
    }

    pub fn clsid(&self) -> GUID {
        let digest = Sha256::digest(format!("PR Sniper toast activator v1:{}", self.profile));
        let mut bytes: [u8; 16] = digest[..16].try_into().unwrap();
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        GUID::from_u128(u128::from_be_bytes(bytes))
    }

    pub fn key(&self) -> String {
        format!("Software\\Classes\\CLSID\\{{{:?}}}", self.clsid())
    }

    pub fn command(&self) -> Result<String, String> {
        Ok(format!(
            "\"{}\" {SERVER_ARG} {}",
            shell_path(&self.executable)?,
            self.encode()?
        ))
    }

    pub fn shortcut(&self) -> Result<PathBuf, String> {
        let folder = known_folder(&FOLDERID_Programs)?;
        let prefix = if self.credential_service.is_some() {
            "test "
        } else {
            ""
        };
        Ok(folder.join(format!(
            "PR Sniper notifications ({prefix}{}).lnk",
            self.profile
        )))
    }

    pub fn notice_id(&self, id: &str) -> Result<(), String> {
        let prefix = format!("pr-sniper:{}:", self.profile);
        let suffix = id
            .strip_prefix(&prefix)
            .ok_or("Notification belongs to a different profile.")?;
        if uuid::Uuid::parse_str(suffix)
            .map(|u| u.to_string() != suffix)
            .unwrap_or(true)
        {
            return Err("Notification activation does not identify a saved notice.".into());
        }
        Ok(())
    }

    /// Read-only: neither normal launch nor permission inspection claims or repairs ownership.
    pub fn registered(&self) -> Result<bool, String> {
        self.validate()?;
        let key = self.key();
        let owner = registry_read(&key, OWNER)?;
        let command = registry_read(&format!("{key}\\LocalServer32"), "")?;
        let executable = registry_read(&format!("{key}\\LocalServer32"), "ServerExecutable")?;
        let shortcut = self.shortcut()?;
        if owner.is_none()
            && command.is_none()
            && executable.is_none()
            && !shortcut
                .try_exists()
                .map_err(|_| "Notification shortcut cannot be inspected.")?
        {
            // An empty or differently populated class key is still foreign.
            if registry_exists(&key)? {
                return Err(
                    "Notification COM identity is occupied by an unowned registration.".into(),
                );
            }
            return Ok(false);
        }
        if owner.as_deref() != Some(self.encode()?.as_str())
            || command.as_deref() != Some(self.command()?.as_str())
            || executable.as_deref() != Some(shell_path(&self.executable)?.as_str())
            || registry_shape(&key)? != Some((1, 1))
            || registry_shape(&format!("{key}\\LocalServer32"))? != Some((0, 2))
            || !self.shortcut_matches(&shortcut)?
        {
            return Err("Notification registration is stale, incomplete or owned by another executable/profile. Nothing was replaced; explicitly remove the old owned registration before opting in again.".into());
        }
        Ok(true)
    }

    pub fn install(&self) -> Result<(), String> {
        if self.registered()? {
            return Ok(());
        }
        let shortcut = self.shortcut()?;
        // Stage beside the target, then link without replacement. Unlike Save,
        // hard_link fails if a shortcut appeared since the ownership inspection.
        let stage = shortcut.with_file_name(format!(
            ".PRSniper-notification-{}.stage",
            uuid::Uuid::new_v4()
        ));
        let reservation = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|_| "Cannot reserve a notification shortcut stage; nothing was replaced.")?;
        drop(reservation);
        let result = self.write_shortcut(&stage).and_then(|()| {
            std::fs::hard_link(&stage, &shortcut).map_err(|_| {
                "Cannot install notification shortcut without replacement.".to_string()
            })
        });
        if std::fs::remove_file(&stage).is_err() {
            return Err(format!("Notification shortcut stage cleanup failed at {}. No other path was removed; inspect this exact stage before retrying.", stage.display()));
        }
        result?;
        // A failed registry setup remains visible and is never mistaken for an
        // installed identity. Preserve the shortcut as evidence for exact recovery.
        self.install_key()?;
        self.registered()?
            .then_some(())
            .ok_or("Notification registration verification failed.".into())
    }

    fn install_key(&self) -> Result<(), String> {
        let key = self.key();
        let (root, created) = RegistryKey::create(&key)?;
        if !created {
            return Err(
                "Notification COM identity appeared during setup; it was not replaced.".into(),
            );
        }
        let result: Result<(), String> = (|| {
            root.write(OWNER, &self.encode()?)?;
            let (server, created) = RegistryKey::create(&format!("{key}\\LocalServer32"))?;
            if !created {
                return Err(
                    "Notification COM server appeared during setup; it was not replaced.".into(),
                );
            }
            server.write("", &self.command()?)?;
            server.write("ServerExecutable", &shell_path(&self.executable)?)?;
            Ok(())
        })();
        drop(root);
        if let Err(error) = result {
            return Err(format!("{error} Partial notification registration remains at HKCU\\{key}; inspect this exact owned key before manual recovery. No existing key was removed."));
        }
        Ok(())
    }

    pub fn uninstall(&self) -> Result<(), String> {
        if !self.registered()? {
            return Ok(());
        }
        std::fs::remove_file(self.shortcut()?)
            .map_err(|_| "Owned notification shortcut could not be removed.")?;
        remove_key_values(
            &format!("{}\\LocalServer32", self.key()),
            &["", "ServerExecutable"],
        )?;
        remove_key_values(&self.key(), &[OWNER])
    }

    fn write_shortcut(&self, path: &Path) -> Result<(), String> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| native_error("shortcut creation", e))?;
            link.SetPath(PCWSTR(wide(&shell_path(&self.executable)?).as_ptr()))
                .map_err(|e| native_error("shortcut executable", e))?;
            link.SetArguments(PCWSTR(
                wide(&format!("{OPEN_ARG} {}", self.encode()?)).as_ptr(),
            ))
            .map_err(|e| native_error("shortcut profile", e))?;
            link.SetDescription(windows::core::w!("PR Sniper notification identity"))
                .map_err(|e| native_error("shortcut description", e))?;
            let properties: IPropertyStore = link
                .cast()
                .map_err(|e| native_error("shortcut properties", e))?;
            properties
                .SetValue(
                    &PKEY_AppUserModel_ID,
                    &PROPVARIANT::from(self.aumid().as_str()),
                )
                .map_err(|e| native_error("shortcut application identity", e))?;
            let clsid = InitPropVariantFromCLSID(&self.clsid())
                .map_err(|e| native_error("shortcut activator", e))?;
            properties
                .SetValue(&PKEY_AppUserModel_ToastActivatorCLSID, &clsid)
                .map_err(|e| native_error("shortcut activator identity", e))?;
            properties
                .Commit()
                .map_err(|e| native_error("shortcut commit", e))?;
            let persist: IPersistFile = link
                .cast()
                .map_err(|e| native_error("shortcut persistence", e))?;
            persist
                .Save(
                    PCWSTR(wide(path.to_str().ok_or("Invalid shortcut path.")?).as_ptr()),
                    true,
                )
                .map_err(|e| native_error("shortcut save", e))
        }
    }

    fn shortcut_matches(&self, path: &Path) -> Result<bool, String> {
        if !path
            .try_exists()
            .map_err(|_| "Notification shortcut cannot be inspected.")?
        {
            return Ok(false);
        }
        if std::fs::symlink_metadata(path)
            .map_err(|_| "Cannot inspect notification shortcut.")?
            .file_type()
            .is_symlink()
        {
            return Ok(false);
        }
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| native_error("shortcut inspection", e))?;
            let persist: IPersistFile = link
                .cast()
                .map_err(|e| native_error("shortcut inspection", e))?;
            persist
                .Load(
                    PCWSTR(wide(path.to_str().ok_or("Invalid shortcut path.")?).as_ptr()),
                    STGM_READ,
                )
                .map_err(|e| native_error("shortcut read", e))?;
            let mut executable = [0u16; 32768];
            let mut args = [0u16; 32768];
            link.GetPath(&mut executable, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                .map_err(|e| native_error("shortcut target read", e))?;
            link.GetArguments(&mut args)
                .map_err(|e| native_error("shortcut arguments read", e))?;
            let properties: IPropertyStore = link
                .cast()
                .map_err(|e| native_error("shortcut properties", e))?;
            let app = properties
                .GetValue(&PKEY_AppUserModel_ID)
                .map_err(|e| native_error("shortcut identity read", e))?;
            let clsid = properties
                .GetValue(&PKEY_AppUserModel_ToastActivatorCLSID)
                .map_err(|e| native_error("shortcut activator read", e))?;
            let actual_clsid = PropVariantToGUID(&clsid)
                .map_err(|e| native_error("shortcut activator type", e))?;
            Ok(PathBuf::from(from_wide(&executable)?)
                .canonicalize()
                .ok()
                .as_ref()
                == Some(&self.executable)
                && from_wide(&args)? == format!("{OPEN_ARG} {}", self.encode()?)
                && app.to_string() == self.aumid()
                && actual_clsid == self.clsid())
        }
    }
}

fn from_wide(value: &[u16]) -> Result<String, String> {
    let end = value
        .iter()
        .position(|v| *v == 0)
        .ok_or("Unterminated Windows notification identity.")?;
    String::from_utf16(&value[..end]).map_err(|_| "Invalid Windows notification identity.".into())
}

struct RegistryKey(HKEY);
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
impl RegistryKey {
    fn create(path: &str) -> Result<(Self, bool), String> {
        let mut key = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide(path).as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                None,
                &mut key,
                Some(&mut disposition),
            )
            .ok()
            .map_err(|e| native_error("registration creation", e))?;
        }
        Ok((Self(key), disposition == REG_CREATED_NEW_KEY))
    }
    fn write(&self, name: &str, value: &str) -> Result<(), String> {
        let bytes: Vec<u8> = wide(value).into_iter().flat_map(u16::to_le_bytes).collect();
        unsafe {
            RegSetValueExW(
                self.0,
                PCWSTR(wide(name).as_ptr()),
                None,
                REG_SZ,
                Some(&bytes),
            )
            .ok()
            .map_err(|e| native_error("registration write", e))
        }
    }
}

fn registry_exists(path: &str) -> Result<bool, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(path).as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    status
        .ok()
        .map_err(|e| native_error("registration inspection", e))?;
    drop(RegistryKey(key));
    Ok(true)
}

fn registry_shape(path: &str) -> Result<Option<(u32, u32)>, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(path).as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status
        .ok()
        .map_err(|e| native_error("registration shape inspection", e))?;
    let key = RegistryKey(key);
    let (mut subkeys, mut values) = (0, 0);
    unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            Some(&mut subkeys),
            None,
            None,
            Some(&mut values),
            None,
            None,
            None,
            None,
        )
        .ok()
        .map_err(|e| native_error("registration ownership inspection", e))?;
    }
    Ok(Some((subkeys, values)))
}

fn registry_read(path: &str, name: &str) -> Result<Option<String>, String> {
    let mut bytes = [0u16; 32768];
    let mut size = std::mem::size_of_val(&bytes) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(path).as_ptr()),
            PCWSTR(wide(name).as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status
        .ok()
        .map_err(|e| native_error("registration read", e))?;
    from_wide(&bytes).map(Some)
}

fn remove_key_values(path: &str, names: &[&str]) -> Result<(), String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(path).as_ptr()),
            None,
            KEY_READ | KEY_WRITE,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    status
        .ok()
        .map_err(|e| native_error("registration cleanup open", e))?;
    let key = RegistryKey(key);
    for name in names {
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(wide(name).as_ptr())) };
        if status != ERROR_FILE_NOT_FOUND {
            status
                .ok()
                .map_err(|e| native_error("registration value cleanup", e))?;
        }
    }
    let mut subkeys = 0;
    let mut values = 0;
    unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            Some(&mut subkeys),
            None,
            None,
            Some(&mut values),
            None,
            None,
            None,
            None,
        )
        .ok()
        .map_err(|e| native_error("registration cleanup inspection", e))?;
    }
    if subkeys != 0 || values != 0 {
        return Err(
            "Notification registration has foreign contents; its key was not removed.".into(),
        );
    }
    drop(key);
    unsafe { RegDeleteKeyExW(HKEY_CURRENT_USER, PCWSTR(wide(path).as_ptr()), 0, None) }
        .ok()
        .map_err(|e| native_error("registration key cleanup", e))
}

#[derive(Debug)]
pub struct PermissionError {
    pub message: String,
    pub needs_initialization: bool,
}

pub fn permission(registration: &Registration) -> Result<NotificationSetting, PermissionError> {
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(registration.aumid()))
        .map_err(|e| PermissionError {
            message: native_error("permission notifier creation", e),
            needs_initialization: false,
        })?
        .Setting()
        .map_err(|e| PermissionError {
            needs_initialization: e.code().0 as u32 == 0x80070490,
            message: native_error("permission Setting read", e),
        })
}

pub const SETUP_GROUP: &str = "permission-setup";

pub fn setup_tag(id: &str) -> String {
    format!("{:x}", Sha256::digest(id.as_bytes()))[..16].into()
}

pub fn remove_setup(registration: &Registration, id: &str) -> Result<(), String> {
    registration.notice_id(id)?;
    ToastNotificationManager::History()
        .and_then(|history| {
            history.RemoveGroupedTagWithId(
                &HSTRING::from(setup_tag(id)),
                &HSTRING::from(SETUP_GROUP),
                &HSTRING::from(registration.aumid()),
            )
        })
        .map_err(|e| native_error("exact setup notification cleanup (outcome unknown)", e))
}

pub fn setup_toast(id: &str, title: &str, body: &str) -> Result<ToastNotification, String> {
    let xml = XmlDocument::new()
        .and_then(|xml| {
            xml.LoadXml(&HSTRING::from(toast_xml(id, title, body)))?;
            Ok(xml)
        })
        .map_err(|e| native_error("setup XML", e))?;
    let toast = ToastNotification::CreateToastNotification(&xml)
        .map_err(|e| native_error("setup request creation", e))?;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Notification setup clock is unavailable.")?
        .as_secs();
    let ticks = seconds
        .checked_add(15)
        .and_then(|s| s.checked_mul(10_000_000))
        .and_then(|t| t.checked_add(116_444_736_000_000_000))
        .and_then(|t| i64::try_from(t).ok())
        .ok_or("Notification setup expiration is unavailable.")?;
    let expiration: windows::Foundation::IReference<windows::Foundation::DateTime> =
        windows::Foundation::PropertyValue::CreateDateTime(windows::Foundation::DateTime {
            UniversalTime: ticks,
        })
        .and_then(|value| value.cast())
        .map_err(|e| native_error("setup expiration", e))?;
    toast
        .SetSuppressPopup(true)
        .and_then(|()| toast.SetTag(&HSTRING::from(setup_tag(id))))
        .and_then(|()| toast.SetGroup(&HSTRING::from(SETUP_GROUP)))
        .and_then(|()| toast.SetExpirationTime(&expiration))
        .map_err(|e| native_error("setup suppression/expiry", e))?;
    Ok(toast)
}

pub fn submit_setup(
    registration: &Registration,
    id: &str,
    title: &str,
    body: &str,
) -> Result<(), (String, bool)> {
    registration.notice_id(id).map_err(|e| (e, false))?;
    let toast = setup_toast(id, title, body).map_err(|e| (e, false))?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(registration.aumid()))
            .map_err(|e| (native_error("setup notifier creation", e), false))?;
    notifier.Show(&toast).map_err(|e| {
        (
            native_error("setup submission (outcome unknown; will not resend)", e),
            true,
        )
    })
}

pub fn authorization(setting: NotificationSetting) -> &'static str {
    match setting {
        NotificationSetting::Enabled => "authorized_aggregate",
        NotificationSetting::DisabledForApplication => "denied",
        NotificationSetting::DisabledForUser => "disabled_for_user",
        NotificationSetting::DisabledByGroupPolicy => "disabled_by_policy",
        NotificationSetting::DisabledByManifest => "disabled_by_manifest",
        _ => "unknown",
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn toast_xml(id: &str, title: &str, body: &str) -> String {
    format!(
        "<toast activationType=\"foreground\" launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual><audio silent=\"true\"/></toast>",
        escape(id), escape(title), escape(body)
    )
}

/// The caller has already persisted send intent. Any Show failure is conservatively uncertain.
pub fn send(
    registration: &Registration,
    id: &str,
    title: &str,
    body: &str,
) -> Result<(), (String, bool)> {
    registration.notice_id(id).map_err(|e| (e, false))?;
    let xml = XmlDocument::new()
        .and_then(|xml| {
            xml.LoadXml(&HSTRING::from(toast_xml(id, title, body)))?;
            Ok(xml)
        })
        .map_err(|e| (native_error("XML preparation", e), false))?;
    let toast = ToastNotification::CreateToastNotification(&xml)
        .map_err(|e| (native_error("request creation", e), false))?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(registration.aumid()))
            .map_err(|e| (native_error("notifier creation", e), false))?;
    notifier.Show(&toast).map_err(|e| {
        (
            native_error("submission (outcome unknown; will not resend)", e),
            true,
        )
    })
}

type Callback = Arc<dyn Fn(String) -> Result<(), String> + Send + Sync>;

pub fn run_relay(
    registration: Registration,
    validate: Callback,
    handoff: Callback,
    receive_timeout: Duration,
) -> Result<(), String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let admitting = Arc::new(AtomicBool::new(true));
    let admission = admitting.clone();
    let server = ActivationServer::start(
        registration,
        Arc::new(move |id| {
            if admission
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return Err(
                    "Notification activation is already being handled; retry this notification."
                        .into(),
                );
            }
            // A callback reports success only after forwarding, never for admission
            // into a queue whose receiver may already be tearing down.
            let result = validate(id.clone()).and_then(|()| handoff(id));
            tx.send(result.clone())
                .map_err(|_| "Notification handoff exceeded its lifetime.")?;
            result
        }),
    )?;
    let result = rx
        .recv_timeout(receive_timeout)
        .map_err(|_| "Windows did not complete a notification handoff before timeout.".to_string())
        .and_then(|result| result);
    admitting.store(false, Ordering::SeqCst);
    combine(result, server.stop())
}

fn combine(operation: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (operation, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(operation), Err(cleanup)) => Err(format!("{operation} {cleanup}")),
    }
}

#[derive(Default)]
struct Lifetime {
    closed: bool,
    objects: usize,
    locks: usize,
}

#[implement(INotificationActivationCallback)]
struct Activation {
    registration: Registration,
    callback: Callback,
    lifetime: Arc<Mutex<Lifetime>>,
}
impl Drop for Activation {
    fn drop(&mut self) {
        match self.lifetime.lock() {
            Ok(mut lifetime) => lifetime.objects -= 1,
            Err(_) => eprintln!("Windows notification object release accounting failed."),
        }
    }
}
impl INotificationActivationCallback_Impl for Activation_Impl {
    fn Activate(
        &self,
        app: &PCWSTR,
        args: &PCWSTR,
        _data: *const NOTIFICATION_USER_INPUT_DATA,
        count: u32,
    ) -> windows::core::Result<()> {
        if app.is_null() || args.is_null() || count != 0 {
            return Err(E_INVALIDARG.into());
        }
        if self
            .lifetime
            .lock()
            .map_err(|_| windows::core::Error::from(E_INVALIDARG))?
            .closed
        {
            return Err(E_INVALIDARG.into());
        }
        let (app, id) = unsafe { (app.to_string()?, args.to_string()?) };
        if app != self.registration.aumid() || self.registration.notice_id(&id).is_err() {
            return Err(E_INVALIDARG.into());
        }
        (self.callback)(id).map_err(|_| windows::core::Error::from(E_INVALIDARG))
    }
}

#[implement(IClassFactory)]
struct Factory {
    registration: Registration,
    callback: Callback,
    lifetime: Arc<Mutex<Lifetime>>,
}
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<windows::core::IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *object = std::ptr::null_mut();
        }
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        {
            let mut lifetime = self
                .lifetime
                .lock()
                .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
            if lifetime.closed {
                return Err(E_INVALIDARG.into());
            }
            lifetime.objects += 1;
        }
        let activation: INotificationActivationCallback = Activation {
            registration: self.registration.clone(),
            callback: self.callback.clone(),
            lifetime: self.lifetime.clone(),
        }
        .into();
        unsafe { activation.query(iid, object).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        let mut lifetime = self
            .lifetime
            .lock()
            .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
        if lock.as_bool() {
            if lifetime.closed {
                return Err(E_INVALIDARG.into());
            }
            lifetime.locks += 1;
        } else {
            lifetime.locks = lifetime
                .locks
                .checked_sub(1)
                .ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
        }
        Ok(())
    }
}

enum ServerCommand {
    Revoke(mpsc::SyncSender<Result<(), String>>),
    Finish,
}

pub struct ActivationServer {
    commands: mpsc::Sender<ServerCommand>,
    thread: Option<thread::JoinHandle<Result<(), String>>>,
    lifetime: Arc<Mutex<Lifetime>>,
}
impl ActivationServer {
    pub fn start(registration: Registration, callback: Callback) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (commands, received) = mpsc::channel();
        let lifetime = Arc::new(Mutex::new(Lifetime::default()));
        let factory_lifetime = lifetime.clone();
        let thread = thread::Builder::new()
            .name("notification-activation".into())
            .spawn(move || {
                let result: Result<_, String> = (|| {
                    let apartment = Apartment::new()?;
                    let factory: IClassFactory = Factory {
                        registration: registration.clone(),
                        callback,
                        lifetime: factory_lifetime,
                    }
                    .into();
                    let cookie = unsafe {
                        CoRegisterClassObject(
                            &registration.clsid(),
                            &factory,
                            CLSCTX_LOCAL_SERVER,
                            REGCLS_MULTIPLEUSE,
                        )
                    }
                    .map_err(|e| native_error("COM activation registration", e))?;
                    Ok((apartment, factory, cookie))
                })();
                match result {
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        Err(error)
                    }
                    Ok((_apartment, _factory, cookie)) => {
                        let _ = ready_tx.send(Ok(()));
                        let mut cookie = Some(cookie);
                        while let Ok(command) = received.recv() {
                            match command {
                                ServerCommand::Revoke(reply) => {
                                    let result = match cookie.take() {
                                        Some(cookie) => unsafe { CoRevokeClassObject(cookie) }
                                            .map_err(|e| {
                                                native_error("COM activation revocation", e)
                                            }),
                                        None => Ok(()),
                                    };
                                    let _ = reply.send(result);
                                }
                                ServerCommand::Finish => break,
                            }
                        }
                        if let Some(cookie) = cookie {
                            unsafe { CoRevokeClassObject(cookie) }
                                .map_err(|e| native_error("COM activation revocation", e))?;
                        }
                        Ok(())
                    }
                }
            })
            .map_err(|_| "Windows notification activation thread could not start.")?;
        // Registration is local native work; no provider or UI operation runs here.
        let result = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "Windows notification activation registration timed out.".to_string())
            .and_then(|v| v);
        if let Err(error) = result {
            let _ = commands.send(ServerCommand::Finish);
            if !matches!(thread.join(), Ok(Ok(()))) {
                return Err(format!(
                    "{error} Notification activation startup teardown also failed."
                ));
            }
            return Err(error);
        }
        Ok(Self {
            commands,
            thread: Some(thread),
            lifetime,
        })
    }
    pub fn wait_released(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let released = {
                let lifetime = self
                    .lifetime
                    .lock()
                    .map_err(|_| "Notification COM lifetime unavailable.")?;
                lifetime.objects == 0 && lifetime.locks == 0
            };
            if released {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(
                    "Windows retained the notification COM callback after its bounded handoff."
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn stop(mut self) -> Result<(), String> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let closed = self
            .lifetime
            .lock()
            .map(|mut lifetime| lifetime.closed = true)
            .map_err(|_| "Notification COM admission could not close.".to_string());
        let (tx, rx) = mpsc::sync_channel(1);
        let revoked = self
            .commands
            .send(ServerCommand::Revoke(tx))
            .map_err(|_| "Notification activation thread is unavailable.".to_string())
            .and_then(|()| {
                rx.recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "Notification COM revocation timed out.".to_string())
            })
            .and_then(|result| result);
        // Keep the MTA alive after revocation while clients release existing
        // objects/locks. No new object, lock, or activation can enter this fence.
        let released = self.wait_released();
        let _ = self.commands.send(ServerCommand::Finish);
        let joined = self
            .thread
            .take()
            .unwrap()
            .join()
            .map_err(|_| "Windows notification activation thread failed.".to_string())
            .and_then(|result| result);
        combine(combine(closed, revoked), combine(released, joined))
    }
}
impl Drop for ActivationServer {
    fn drop(&mut self) {
        if self.thread.is_some() {
            if let Err(error) = self.shutdown() {
                eprintln!("{error}");
            }
        }
    }
}
