//! Inbox Windows APIs. Persistent registration is separate from process lifetime.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
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

pub fn permission(registration: &Registration) -> Result<NotificationSetting, String> {
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(registration.aumid()))
        .and_then(|notifier| notifier.Setting())
        .map_err(|e| native_error("permission read", e))
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

#[implement(INotificationActivationCallback)]
struct Activation {
    registration: Registration,
    callback: Callback,
    objects: Arc<AtomicUsize>,
}
impl Drop for Activation {
    fn drop(&mut self) {
        self.objects.fetch_sub(1, Ordering::SeqCst);
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
    objects: Arc<AtomicUsize>,
    locks: Arc<AtomicUsize>,
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
        self.objects.fetch_add(1, Ordering::SeqCst);
        let activation: INotificationActivationCallback = Activation {
            registration: self.registration.clone(),
            callback: self.callback.clone(),
            objects: self.objects.clone(),
        }
        .into();
        unsafe { activation.query(iid, object).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            self.locks.fetch_add(1, Ordering::SeqCst);
        } else {
            self.locks
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
        }
        Ok(())
    }
}

pub struct ActivationServer {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<Result<(), String>>>,
    objects: Arc<AtomicUsize>,
    locks: Arc<AtomicUsize>,
}
impl ActivationServer {
    pub fn start(registration: Registration, callback: Callback) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop, stopped) = mpsc::channel();
        let objects = Arc::new(AtomicUsize::new(0));
        let factory_objects = objects.clone();
        let locks = Arc::new(AtomicUsize::new(0));
        let factory_locks = locks.clone();
        let thread = thread::Builder::new()
            .name("notification-activation".into())
            .spawn(move || {
                let result: Result<_, String> = (|| {
                    let apartment = Apartment::new()?;
                    let factory: IClassFactory = Factory {
                        registration: registration.clone(),
                        callback,
                        objects: factory_objects,
                        locks: factory_locks,
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
                        let _ = stopped.recv();
                        unsafe { CoRevokeClassObject(cookie) }
                            .map_err(|e| native_error("COM activation revocation", e))
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
            let _ = stop.send(());
            return Err(error);
        }
        Ok(Self {
            stop,
            thread: Some(thread),
            objects,
            locks,
        })
    }
    pub fn wait_released(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while self.objects.load(Ordering::SeqCst) != 0 || self.locks.load(Ordering::SeqCst) != 0 {
            if std::time::Instant::now() >= deadline {
                return Err(
                    "Windows retained the notification COM callback after its bounded handoff."
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
    pub fn stop(mut self) -> Result<(), String> {
        let _ = self.stop.send(());
        self.thread
            .take()
            .unwrap()
            .join()
            .map_err(|_| "Windows notification activation thread failed.")?
    }
}
impl Drop for ActivationServer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            if !matches!(thread.join(), Ok(Ok(()))) {
                eprintln!("Windows notification activation teardown failed.");
            }
        }
    }
}
