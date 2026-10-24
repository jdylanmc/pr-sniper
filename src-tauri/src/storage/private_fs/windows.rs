use std::ffi::{c_void, OsStr};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, SetFileSecurityW, TokenUser, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    },
    Storage::FileSystem::CreateDirectoryW,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // Both conversion APIs transfer a LocalAlloc-owned allocation.
        unsafe { LocalFree(self.0) };
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    value.push(0);
    Ok(value)
}

/// Raw Win32 calls bypass std's long-path conversion; without it, a path at
/// MAX_PATH fails here even though std created it. Mirror std: absolute,
/// normalized, extended-length form; existing verbatim/device paths pass through.
pub(super) fn extended_path(path: &Path) -> io::Result<Vec<u16>> {
    const VERBATIM: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const DEVICE: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'.' as u16, b'\\' as u16];
    let raw: Vec<u16> = path.as_os_str().encode_wide().collect();
    if raw.starts_with(&VERBATIM) || raw.starts_with(&DEVICE) {
        return wide(path.as_os_str());
    }
    let absolute: Vec<u16> = std::path::absolute(path)?
        .as_os_str()
        .encode_wide()
        .collect();
    let mut extended: Vec<u16> = if let Some(share) = absolute.strip_prefix(&VERBATIM[..2]) {
        r"\\?\UNC\"
            .encode_utf16()
            .chain(share.iter().copied())
            .collect()
    } else {
        r"\\?\".encode_utf16().chain(absolute).collect()
    };
    if extended.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    extended.push(0);
    Ok(extended)
}

fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn user_sid() -> io::Result<String> {
    // TOKEN_USER's SID points into the aligned token-information allocation;
    // keep that allocation alive until the SID has been converted.
    unsafe {
        let mut token = null_mut();
        check(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        ))?;
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length);
        if length < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        check(GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ))?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = null_mut();
        check(ConvertSidToStringSidW(user.User.Sid, &mut sid))?;
        let _allocation = LocalAllocation(sid.cast());
        let mut length = 0;
        while *sid.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(sid, length))
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }
}

fn descriptor(directory: bool) -> io::Result<LocalAllocation> {
    let inheritance = if directory { "OICI" } else { "" };
    let sddl = wide(OsStr::new(&format!(
        "D:P(A;{inheritance};FA;;;{})",
        user_sid()?
    )))?;
    let mut descriptor = null_mut();
    // Protected DACL: no inherited broad grants; only the current user gets
    // full access. Directory grants propagate to newly created child files.
    unsafe {
        check(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        ))?;
    }
    Ok(LocalAllocation(descriptor))
}

pub(super) fn create_directory(path: &Path) -> io::Result<()> {
    let path = extended_path(path)?;
    let descriptor = descriptor(true)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    unsafe { check(CreateDirectoryW(path.as_ptr(), &attributes)) }
}

pub(super) fn protect(path: &Path, directory: bool) -> io::Result<()> {
    let path = extended_path(path)?;
    let descriptor = descriptor(directory)?;
    unsafe {
        check(SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        ))
    }
}
