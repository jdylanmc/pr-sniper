use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

#[cfg(windows)]
#[path = "private_fs/windows.rs"]
mod windows;

fn reject_link(metadata: &Metadata) -> io::Result<()> {
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "linked storage",
        ));
    }
    Ok(())
}

pub(super) fn existing_directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "relative storage"));
    }
    let metadata = fs::symlink_metadata(path)?;
    reject_link(&metadata)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotADirectory,
            "storage directory",
        ));
    }
    Ok(())
}

pub(super) fn directory(path: &Path) -> io::Result<()> {
    match existing_directory(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::from(ErrorKind::InvalidInput))?;
            if !parent.exists() {
                directory(parent)?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new().mode(0o700).create(path)?;
            }
            #[cfg(windows)]
            windows::create_directory(path)?;
        }
        Err(error) => return Err(error),
    }
    #[cfg(windows)]
    windows::protect(path, true)?;
    Ok(())
}

pub(super) fn existing_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    reject_link(&metadata)?;
    if !metadata.is_file() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "storage file"));
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> io::Result<Vec<u8>> {
    existing_file(path)?;
    fs::read(path)
}

pub(super) fn append(path: &Path) -> io::Result<File> {
    match existing_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(windows)]
    windows::protect(path, false)?;
    Ok(file)
}

pub(super) fn rotate(path: &Path, previous: &Path) -> io::Result<()> {
    existing_file(path)?;
    #[cfg(windows)]
    windows::protect(path, false)?;
    fs::rename(path, previous)
}

pub(super) fn replace(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    let temporary = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .expect("owned storage filename")
            .to_string_lossy()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // The Windows parent has an inheritable current-user-only ACL before
    // creation. Never truncate or remove an unowned, pre-existing stage.
    let mut file = options
        .open(&temporary)
        .map_err(|_| format!("Cannot write {label}."))?;
    let flushed = (|| {
        #[cfg(windows)]
        windows::protect(&temporary, false)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    // Windows rename/replacement must not retain the staging handle.
    drop(file);
    let result = flushed
        .map_err(|_| format!("Cannot flush {label}."))
        .and_then(|_| {
            match existing_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(_) => return Err(format!("Cannot replace {label}.")),
            }
            fs::rename(&temporary, path).map_err(|_| format!("Cannot replace {label}."))
        });
    if result.is_err() && fs::remove_file(&temporary).is_err() {
        return Err(format!("Cannot replace or clean up staged {label}."));
    }
    result
}
