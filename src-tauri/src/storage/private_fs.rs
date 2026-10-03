use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
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

pub(crate) fn directory(path: &Path) -> io::Result<()> {
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

fn create_stage(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    // File sync and rename cover process interruption on Windows. Do not claim
    // portable directory durability across power loss.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

const WRITE_MARKER: &str = ".pr-sniper-write-";

#[cfg(test)]
#[path = "private_fs/tests.rs"]
mod tests;

fn ownership(target: &str, nonce: &str) -> String {
    format!("pr-sniper-replace-v1\n{target}\n{nonce}\n")
}

fn owned_file(path: &Path) -> io::Result<()> {
    existing_file(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path)?;
        if metadata.nlink() != 1 || metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                ErrorKind::PermissionDenied,
                "non-private stage",
            ));
        }
    }
    Ok(())
}

fn discard_stage(path: &Path) -> io::Result<()> {
    match owned_file(path) {
        Ok(()) => fs::remove_file(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Discard only payloads with a complete, private ownership record. The writer
/// locks that record until it has renamed or discarded its payload.
pub(crate) fn recover(directory: &Path, target: Option<&str>) -> io::Result<()> {
    match existing_directory(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some((file, nonce)) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".owner"))
            .and_then(|n| n.rsplit_once(WRITE_MARKER))
        else {
            continue;
        };
        if target.is_some_and(|target| target != file) || uuid::Uuid::parse_str(nonce).is_err() {
            continue;
        }
        let marker = entry.path();
        owned_file(&marker)?;
        let mut owner = OpenOptions::new().read(true).write(true).open(&marker)?;
        owner.try_lock().map_err(io::Error::other)?;
        let expected = ownership(file, nonce);
        let mut actual = Vec::new();
        (&mut owner)
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        let stage = directory.join(format!("{file}{WRITE_MARKER}{nonce}.stage"));
        if actual != expected.as_bytes() {
            // Only an empty stage plus a partial claim can be a bootstrap
            // interruption. Corrupt proof around payload data is a visible error.
            owned_file(&stage)?;
            if !expected.as_bytes().starts_with(&actual) || fs::metadata(&stage)?.len() != 0 {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "invalid stage ownership",
                ));
            }
            continue;
        }
        discard_stage(&stage)?;
        #[cfg(test)]
        recovery_fault::check(&directory.join(file), "sync")?;
        sync_directory(directory)?;
        drop(owner);
        #[cfg(test)]
        recovery_fault::check(&directory.join(file), "unlink")?;
        fs::remove_file(marker)?;
        sync_directory(directory)?;
    }
    Ok(())
}

pub(super) fn replace(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Cannot write {label}."))?;
    let directory = path
        .parent()
        .ok_or_else(|| format!("Cannot write {label}."))?;
    // Old fixed stages have no ownership proof. Preserve and refuse them, even
    // when empty; never infer ownership from a suffix or truncate an occupant.
    match fs::symlink_metadata(path.with_file_name(format!("{name}.tmp"))) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        _ => return Err(format!("Cannot write {label}.")),
    }
    recover(directory, Some(name)).map_err(|_| format!("Cannot recover staged {label}."))?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let temporary = directory.join(format!("{name}{WRITE_MARKER}{nonce}.stage"));
    let marker = directory.join(format!("{name}{WRITE_MARKER}{nonce}.owner"));
    let mut file = create_stage(&temporary).map_err(|_| format!("Cannot write {label}."))?;
    #[cfg(test)]
    crash_test::boundary(path, &temporary, "bootstrap-stage");
    // Claim only a stage this invocation created successfully. Before the claim
    // is durable, the stage stays empty, including every bootstrap crash window.
    let mut owner = match create_stage(&marker) {
        Ok(owner) => owner,
        Err(_) => {
            drop(file);
            fs::remove_file(&temporary).map_err(|_| format!("Cannot clean up staged {label}."))?;
            return Err(format!("Cannot claim staged {label}."));
        }
    };
    let mut claimed = false;
    let flushed = (|| {
        owner.try_lock().map_err(io::Error::other)?;
        #[cfg(test)]
        crash_test::boundary(path, &temporary, "owner-created");
        #[cfg(windows)]
        {
            windows::protect(&temporary, false)?;
            windows::protect(&marker, false)?;
        }
        let proof = ownership(name, &nonce);
        let middle = proof.len() / 2;
        owner.write_all(&proof.as_bytes()[..middle])?;
        #[cfg(test)]
        crash_test::boundary(path, &temporary, "owner-partial");
        #[cfg(test)]
        recovery_fault::check(path, "claim")?;
        owner.write_all(&proof.as_bytes()[middle..])?;
        owner.sync_all()?;
        sync_directory(directory)?;
        claimed = true;
        #[cfg(test)]
        crash_test::boundary(path, &temporary, "created");
        #[cfg(test)]
        {
            let middle = bytes.len() / 2;
            file.write_all(&bytes[..middle])?;
            crash_test::boundary(path, &temporary, "partial");
            recovery_fault::check(path, "payload")?;
            file.write_all(&bytes[middle..])?;
        }
        #[cfg(not(test))]
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(test)]
        crash_test::boundary(path, &temporary, "flushed");
        Ok::<_, io::Error>(())
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
            #[cfg(test)]
            crash_test::boundary(path, &temporary, "pre-rename");
            fs::rename(&temporary, path).map_err(|_| format!("Cannot replace {label}."))
        });
    // Before the full claim is flushed, keep the empty bootstrap stage:
    // removing it alone leaves a partial claim recovery cannot verify.
    if result.is_err() && claimed {
        discard_stage(&temporary)
            .map_err(|_| format!("Cannot replace or clean up staged {label}."))?;
    }
    // Rename is the commit point. No fallible housekeeping may turn a committed
    // save into Err: callers use Err to roll back related configuration. Keep
    // the private claim for the next write or explicit recovery; after rename
    // it owns no payload. Committed reads never perform this housekeeping.
    drop(owner);
    result
}

#[cfg(test)]
pub(crate) mod recovery_fault {
    use super::*;
    use std::{cell::RefCell, path::PathBuf};

    thread_local! {
        static FAIL: RefCell<Option<(PathBuf, &'static str)>> = const { RefCell::new(None) };
    }

    pub(crate) fn arm(path: PathBuf, point: &'static str) {
        FAIL.with(|fault| *fault.borrow_mut() = Some((path, point)));
    }

    pub(super) fn check(path: &Path, point: &str) -> io::Result<()> {
        FAIL.with(|fault| {
            let mut fault = fault.borrow_mut();
            if fault
                .as_ref()
                .is_some_and(|(p, at)| p == path && *at == point)
            {
                *fault = None;
                Err(io::Error::other("injected deferred recovery failure"))
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod crash_test {
    use super::*;
    use std::{cell::RefCell, path::PathBuf};

    thread_local! {
        static STOP: RefCell<Option<(PathBuf, String, usize)>> = const { RefCell::new(None) };
    }

    pub(crate) fn arm(path: PathBuf, boundary: String, occurrence: usize) {
        STOP.with(|stop| *stop.borrow_mut() = Some((path, boundary, occurrence)));
    }

    pub(super) fn boundary(path: &Path, stage: &Path, point: &str) {
        let stop = STOP.with(|stop| {
            let mut stop = stop.borrow_mut();
            let Some((target, boundary, remaining)) = stop.as_mut() else {
                return false;
            };
            if target != path || boundary != point {
                return false;
            }
            *remaining -= 1;
            *remaining == 0
        });
        if stop {
            let root = path.parent().unwrap().parent().unwrap();
            fs::write(
                root.join("crash-ready.pending"),
                serde_json::to_vec(&serde_json::json!({
                    "pid": std::process::id(), "stage": stage, "point": point,
                }))
                .unwrap(),
            )
            .unwrap();
            fs::rename(
                root.join("crash-ready.pending"),
                root.join("crash-ready.json"),
            )
            .unwrap();
            loop {
                std::thread::park();
            }
        }
    }
}
