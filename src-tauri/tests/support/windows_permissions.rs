use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo,
            SDDL_REVISION_1, SE_FILE_OBJECT,
        },
        GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, READ_CONTROL, WRITE_DAC},
};

// Keep a pre-authorized handle so even a failed assertion can restore the
// fixture's original ACL. This never changes the user's profile or host ACLs.
pub struct DeniedAccess {
    handle: File,
    original: *mut c_void,
    dacl: *mut ACL,
}

impl DeniedAccess {
    pub fn permission_changes(path: &Path) -> Self {
        Self::new(path, "D:P(D;;WD;;;OW)(A;OICI;FA;;;OW)")
    }

    pub fn read_data(path: &Path) -> Self {
        Self::new(path, "D:P(D;;0x1;;;OW)(A;OICI;FA;;;OW)")
    }

    fn new(path: &Path, sddl: &str) -> Self {
        let handle = OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap();
        unsafe {
            let mut original = null_mut();
            let mut dacl = null_mut();
            assert_eq!(
                GetSecurityInfo(
                    handle.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    &mut dacl,
                    null_mut(),
                    &mut original,
                ),
                0
            );
            let guard = Self {
                handle,
                original,
                dacl,
            };
            let sddl: Vec<_> = sddl.encode_utf16().chain([0]).collect();
            let mut denied = null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut denied,
                    null_mut(),
                ),
                0
            );
            let mut present = 0;
            let mut defaulted = 0;
            let mut denied_dacl = null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(denied, &mut present, &mut denied_dacl, &mut defaulted,),
                0
            );
            let status = SetSecurityInfo(
                guard.handle.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                denied_dacl,
                null_mut(),
            );
            LocalFree(denied);
            assert_eq!(status, 0);
            guard
        }
    }
}

impl Drop for DeniedAccess {
    fn drop(&mut self) {
        unsafe {
            let status = SetSecurityInfo(
                self.handle.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                self.dacl,
                null_mut(),
            );
            LocalFree(self.original);
            assert_eq!(status, 0, "restore only this fixture's ACL");
        }
    }
}

pub fn assert_private(path: &Path, directory: bool) {
    // Independent OS oracle (.NET ACL reader), not the production descriptor
    // builder. Assert actual disk ACL and the process identity without logging it.
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", r#"
            $ErrorActionPreference = 'Stop'
            $acl = if ($env:PR_SNIPER_ACL_DIRECTORY -eq 'true') {
                [System.IO.Directory]::GetAccessControl($env:PR_SNIPER_ACL_FIXTURE)
            } else {
                [System.IO.File]::GetAccessControl($env:PR_SNIPER_ACL_FIXTURE)
            }

            $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
            $rules = @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
            if (-not $acl.AreAccessRulesProtected) { throw 'DACL not protected' }
            if ($rules.Count -ne 1) { throw 'Unexpected access entries' }
            $rule = $rules[0]
            if ($rule.IdentityReference -ne $sid -or $rule.AccessControlType -ne 'Allow' -or
                $rule.FileSystemRights -ne 'FullControl' -or $rule.IsInherited) {
                throw 'Unexpected access grant'
            }
            $inheritance = if ($env:PR_SNIPER_ACL_DIRECTORY -eq 'true') { 3 } else { 0 }
            if ([int]$rule.InheritanceFlags -ne $inheritance) { throw 'Unexpected inheritance' }
        "#])
        .env("PR_SNIPER_ACL_FIXTURE", path)
        .env("PR_SNIPER_ACL_DIRECTORY", if directory { "true" } else { "false" })
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native ACL verification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn junction(target: &Path, link: &Path) {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "New-Item -ItemType Junction -Path $env:PR_SNIPER_LINK -Target $env:PR_SNIPER_TARGET -ErrorAction Stop | Out-Null"])
        .env_remove("PSModulePath")
        .env("PR_SNIPER_LINK", link)
        .env("PR_SNIPER_TARGET", target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "create fixture junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
