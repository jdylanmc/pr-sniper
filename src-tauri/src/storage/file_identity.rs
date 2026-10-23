//! OS identity of a saved file, cheap enough to check on every guarded read.
//!
//! App writes replace files atomically, so each yields a new inode (Unix) or
//! file ID (Windows). An in-place write by another process changes the
//! kernel-maintained change time, which callers cannot set. A file changed
//! within `RACY` of the check may still share its change time with a later
//! write in the same timestamp tick, so it is never treated as settled.

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Longer than the coarsest change-time tick of supported file systems.
const RACY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    volume: u64,
    file: u128,
    len: u64,
    modified: i128,
    changed: i128,
}

impl Identity {
    /// Whether the file last changed long enough before `now` that any later
    /// change gets a different change time.
    pub(crate) fn settled(&self, now: SystemTime) -> bool {
        now.duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|now| now.checked_sub(RACY))
            .is_some_and(|limit| (limit.as_nanos() as i128) > self.changed.max(self.modified))
    }
}

/// `None` when `path` does not exist.
pub(crate) fn identity(path: &Path) -> io::Result<Option<Identity>> {
    match platform(path) {
        Ok(identity) => Ok(Some(identity)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn platform(path: &Path) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    let nanos = |seconds: i64, nanos: i64| i128::from(seconds) * 1_000_000_000 + i128::from(nanos);
    Ok(Identity {
        volume: metadata.dev(),
        file: u128::from(metadata.ino()),
        len: metadata.size(),
        modified: nanos(metadata.mtime(), metadata.mtime_nsec()),
        changed: nanos(metadata.ctime(), metadata.ctime_nsec()),
    })
}

#[cfg(windows)]
fn platform(path: &Path) -> io::Result<Identity> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, FileIdInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)?;
    let handle = file.as_raw_handle();
    // SAFETY: both structs are plain data, sized exactly for their class, and
    // the handle stays open for both calls.
    let (id, basic) = unsafe {
        let mut id: FILE_ID_INFO = std::mem::zeroed();
        let mut basic: FILE_BASIC_INFO = std::mem::zeroed();
        if GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        ) == 0
            || GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        (id, basic)
    };
    // FILETIME: 100 ns ticks since 1601-01-01.
    const EPOCH_TICKS: i128 = 116_444_736_000_000_000;
    let nanos = |ticks: i64| (i128::from(ticks) - EPOCH_TICKS) * 100;
    Ok(Identity {
        volume: id.VolumeSerialNumber,
        file: u128::from_le_bytes(id.FileId.Identifier),
        len: file.metadata()?.len(),
        modified: nanos(basic.LastWriteTime),
        changed: nanos(basic.ChangeTime),
    })
}

/// Stable identity (volume, file ID) of a real directory. Unlike [`identity`],
/// it ignores times, which change whenever an entry inside is written.
pub(crate) fn directory_identity(path: &Path) -> io::Result<(u64, u128)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir() {
            return Err(io::Error::from(io::ErrorKind::NotADirectory));
        }
        Ok((metadata.dev(), u128::from(metadata.ino())))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandleEx, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let directory = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::from(io::ErrorKind::NotADirectory));
        }
        // SAFETY: FILE_ID_INFO is plain data sized exactly for its class, and
        // the handle stays open for the call.
        let id = unsafe {
            let mut id: FILE_ID_INFO = std::mem::zeroed();
            if GetFileInformationByHandleEx(
                directory.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            id
        };
        Ok((
            id.VolumeSerialNumber,
            u128::from_le_bytes(id.FileId.Identifier),
        ))
    }
}

/// Test support: rewrite `path` as another process might, keeping its length
/// and restoring its modification time, then wait until the change settles.
#[cfg(test)]
pub(crate) fn rewrite_foreign(path: &Path, atomic: bool, edit: impl FnOnce(&mut Vec<u8>)) {
    use std::io::Write;
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    let len = bytes.len();
    edit(&mut bytes);
    assert_eq!(bytes.len(), len);
    let file = if atomic {
        let staged = path.with_extension("foreign");
        std::fs::write(&staged, &bytes).unwrap();
        std::fs::rename(&staged, path).unwrap();
        std::fs::File::options().write(true).open(path).unwrap()
    } else {
        let mut file = std::fs::File::options().write(true).open(path).unwrap();
        file.write_all(&bytes).unwrap();
        file
    };
    file.set_modified(modified).unwrap();
    drop(file);
    assert_eq!(
        std::fs::metadata(path).unwrap().modified().unwrap(),
        modified
    );
    settle();
}

/// Test support: wait out the racy window so identities are trusted again.
#[cfg(test)]
pub(crate) fn settle() {
    std::thread::sleep(RACY + Duration::from_millis(50));
}
