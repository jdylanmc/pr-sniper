use super::*;
use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    path::PathBuf,
};

const PREFIX: &str = ".pr-sniper-package-";
const MANIFEST: &str = ".pr-sniper-install.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    revision: String,
    manifest: Manifest,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Staging,
    Ready,
    Activated,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    target: PathBuf,
    nonce: String,
    phase: Phase,
    before: Option<Proof>,
    after: Option<Proof>,
}

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn conflict() -> Error {
    Error::new(
        "installation_conflict",
        "Package transaction contains changed or unowned content.",
    )
}

fn present(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn workspace(target: &Path) -> Result<PathBuf, Error> {
    Ok(target
        .parent()
        .and_then(Path::parent)
        .ok_or_else(conflict)?
        .join(".pr-sniper-installer"))
}

pub(super) fn locked<T>(
    target: &Path,
    create: bool,
    action: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    verify_parents(target)?;
    let parent = target.parent().ok_or_else(conflict)?;
    let work = workspace(target)?;
    verify_parents(&work)?;
    // Keep package copies outside the host's skill-discovery directory.
    // The lock also serializes status recovery with install/update/removal.
    private_directory(&work).map_err(io_error)?;
    if create {
        private_fs::directory(parent).map_err(io_error)?;
    }
    private_fs::sync_directory(work.parent().unwrap()).map_err(io_error)?;
    let path = work.join(".pr-sniper-install.lock");
    let file = match private_fs::create_file(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            private_fs::existing_file(&path).map_err(|_| conflict())?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(io_error)?
        }
        Err(error) => return Err(io_error(error)),
    };
    file.try_lock()
        .map_err(|_| Error::new("installation_busy", ()))?;
    let _lock = Lock(file);
    action()
}

fn proof(directory: &Path) -> Result<Option<Proof>, Error> {
    let installed = inspect_at(directory)?;
    let Some(revision) = installed.revision else {
        return Ok(None);
    };
    let manifest = serde_json::from_slice(&fs::read(directory.join(MANIFEST)).map_err(io_error)?)
        .map_err(|_| conflict())?;
    Ok(Some(Proof { revision, manifest }))
}

impl Journal {
    fn sibling(&self, suffix: &str) -> PathBuf {
        workspace(&self.target)
            .unwrap()
            .join(format!("{PREFIX}{}{suffix}", self.nonce))
    }
    fn path(&self) -> PathBuf {
        self.sibling(".json")
    }
    fn stage(&self) -> PathBuf {
        self.sibling(".new")
    }
    fn backup(&self) -> PathBuf {
        self.sibling(".old")
    }
    fn write(&self) -> Result<(), Error> {
        let path = self.path();
        private_fs::replace(
            &path,
            &serde_json::to_vec(self).map_err(io_error)?,
            "installation transaction",
        )
        .map_err(io_error)?;
        // Flush the journal's committed rename before any directory move.
        private_fs::sync_directory(path.parent().unwrap()).map_err(io_error)?;
        private_fs::recover(
            path.parent().unwrap(),
            path.file_name().and_then(|n| n.to_str()),
        )
        .map_err(io_error)
    }
    fn clear(&self) -> Result<(), Error> {
        private_fs::existing_file(&self.path()).map_err(|_| conflict())?;
        fs::remove_file(self.path()).map_err(io_error)?;
        private_fs::sync_directory(self.path().parent().unwrap()).map_err(io_error)?;
        checkpoint("journal-removed")
    }
}

fn matches(directory: &Path, expected: Option<&Proof>) -> Result<bool, Error> {
    Ok(proof(directory)?.as_ref() == expected)
}

fn move_directory(source: &Path, target: &Path) -> Result<(), Error> {
    verify_parents(source)?;
    verify_parents(target)?;
    private_fs::existing_directory(source).map_err(|_| conflict())?;
    if present(target)? {
        return Err(conflict());
    }
    fs::rename(source, target).map_err(io_error)?;
    private_fs::sync_directory(target.parent().unwrap()).map_err(io_error)?;
    if source.parent() != target.parent() {
        private_fs::sync_directory(source.parent().unwrap()).map_err(io_error)?;
    }
    Ok(())
}

/// Only journal-owned, hash-matching remnants may be removed. Missing files
/// are expected after interrupted cleanup; extra/changed/linked files are not.
fn discard(directory: &Path, expected: &Proof, label: &str) -> Result<(), Error> {
    if !present(directory)? {
        return Ok(());
    }
    let files = file_hashes(directory)?;
    for (name, hash) in &files {
        let wanted = if name == MANIFEST {
            Some(&expected.revision)
        } else {
            expected.manifest.files.get(name)
        };
        if wanted != Some(hash) {
            return Err(conflict());
        }
    }
    for (name, hash) in files {
        let file = directory.join(name);
        private_fs::existing_file(&file).map_err(|_| conflict())?;
        if revision(&fs::read(&file).map_err(io_error)?) != hash {
            return Err(conflict());
        }
        fs::remove_file(file).map_err(io_error)?;
        checkpoint(label)?;
    }
    for name in ["references", "scripts"] {
        let child = directory.join(name);
        if present(&child)? {
            private_fs::existing_directory(&child).map_err(|_| conflict())?;
            fs::remove_dir(child).map_err(io_error)?;
        }
    }
    fs::remove_dir(directory).map_err(io_error)?;
    private_fs::sync_directory(directory.parent().unwrap()).map_err(io_error)
}

fn finish(journal: &mut Journal) -> Result<(), Error> {
    let target = &journal.target;
    let stage = journal.stage();
    let backup = journal.backup();
    if journal.phase == Phase::Staging {
        if !matches(target, journal.before.as_ref())? || present(&backup)? {
            return Err(conflict());
        }
        discard(
            &stage,
            journal.after.as_ref().ok_or_else(conflict)?,
            "stage-cleanup-file",
        )?;
        return journal.clear();
    }
    if journal.phase == Phase::Ready {
        let live = proof(target)?;
        let already_moved = live.as_ref() == journal.after.as_ref() && !present(&stage)?;
        if !already_moved && live.as_ref() == journal.before.as_ref() && live.is_some() {
            if present(&backup)? {
                return Err(conflict());
            }
            if let Some(after) = &journal.after {
                if !matches(&stage, Some(after))? {
                    return Err(conflict());
                }
            } else if present(&stage)? {
                return Err(conflict());
            }
            move_directory(target, &backup)?;
            checkpoint("old-moved")?;
        } else if live.is_some() && live.as_ref() != journal.after.as_ref() {
            return Err(conflict());
        }
        if !present(target)? {
            if let Some(before) = &journal.before {
                if !matches(&backup, Some(before))? {
                    return Err(conflict());
                }
            } else if present(&backup)? {
                return Err(conflict());
            }
            if let Some(after) = &journal.after {
                if !present(&stage)? {
                    // A missing new package cannot justify deleting the old one.
                    if journal.before.is_some() {
                        move_directory(&backup, target)?;
                        journal.clear()?;
                        return Ok(());
                    }
                    journal.clear()?;
                    return Ok(());
                }
                let stage_valid = matches(&stage, Some(after));
                if !stage_valid.as_ref().is_ok_and(|valid| *valid) {
                    if journal.before.is_some() {
                        move_directory(&backup, target)?;
                        journal.phase = Phase::Staging;
                        journal.write()?;
                    }
                    return Err(stage_valid.err().unwrap_or_else(conflict));
                }
                move_directory(&stage, target)?;
                checkpoint("new-moved")?;
            }
        }
        if !matches(target, journal.after.as_ref())? || present(&stage)? {
            return Err(conflict());
        }
        if let Some(before) = &journal.before {
            if !matches(&backup, Some(before))? {
                return Err(conflict());
            }
        } else if present(&backup)? {
            return Err(conflict());
        }
        journal.phase = Phase::Activated;
        journal.write()?;
        checkpoint("activated")?;
    }
    if !matches(target, journal.after.as_ref())? || present(&stage)? {
        return Err(conflict());
    }
    if let Some(before) = &journal.before {
        discard(&backup, before, "backup-cleanup-file")?;
    } else if present(&backup)? {
        return Err(conflict());
    }
    checkpoint("backup-removed")?;
    journal.clear()
}

pub(super) fn recover(directory: &Path) -> Result<(), Error> {
    let work = workspace(directory)?;
    if !present(&work)? {
        return Ok(());
    }
    for entry in fs::read_dir(&work).map_err(io_error)? {
        let path = entry.map_err(io_error)?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(nonce) = name
            .strip_prefix(PREFIX)
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        let Ok(nonce) = uuid::Uuid::parse_str(nonce) else {
            continue;
        };
        private_fs::owned_file(&path).map_err(|_| conflict())?;
        if fs::metadata(&path).map_err(io_error)?.len() > super::super::MAX_FRAME as u64 {
            return Err(conflict());
        }
        let mut journal: Journal =
            serde_json::from_slice(&fs::read(&path).map_err(io_error)?).map_err(|_| conflict())?;
        if journal.target != directory {
            continue;
        }
        if journal.nonce != nonce.to_string()
            || journal.path() != path
            || journal.before.is_none() && journal.after.is_none()
        {
            return Err(conflict());
        }
        for proof in journal.before.iter().chain(journal.after.iter()) {
            let names: std::collections::BTreeSet<_> = ASSETS
                .iter()
                .map(|(name, _)| *name)
                .chain(std::iter::once("launcher.json"))
                .collect();
            if proof.manifest.protocol != PROTOCOL
                || proof
                    .manifest
                    .files
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>()
                    != names
                || std::iter::once(&proof.revision)
                    .chain(proof.manifest.files.values())
                    .any(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(conflict());
            }
        }
        finish(&mut journal)?;
    }
    Ok(())
}

fn nonce() -> uuid::Uuid {
    #[cfg(test)]
    if let Some(nonce) = tests::next_nonce() {
        return nonce;
    }
    uuid::Uuid::new_v4()
}

pub(super) fn install(
    directory: &Path,
    contents: &[(String, String)],
    expected: Option<&str>,
) -> Result<(), Error> {
    let manifest = Manifest {
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").into(),
        files: contents
            .iter()
            .map(|(name, body)| (name.clone(), revision(&body.as_bytes())))
            .collect(),
    };
    let bytes = serde_json::to_vec(&manifest).map_err(io_error)?;
    let after = Proof {
        revision: revision(&bytes),
        manifest,
    };
    let mut journal = Journal {
        target: directory.into(),
        nonce: nonce().to_string(),
        phase: Phase::Staging,
        before: proof(directory)?,
        after: Some(after),
    };
    if journal.before.as_ref().map(|p| p.revision.as_str()) != expected {
        return Err(Error::new("conflict", ()));
    }
    for path in [journal.path(), journal.stage(), journal.backup()] {
        if present(&path)? {
            return Err(conflict());
        }
    }
    private_fs::create_directory(&journal.stage()).map_err(io_error)?;
    checkpoint("stage-created")?;
    if present(&journal.path())? {
        return Err(conflict());
    }
    journal.write()?;
    checkpoint("journal-written")?;
    for (index, (name, body)) in contents.iter().enumerate() {
        let file = journal.stage().join(name);
        private_fs::directory(file.parent().unwrap()).map_err(io_error)?;
        private_fs::replace(&file, body.as_bytes(), "staged skill asset").map_err(io_error)?;
        checkpoint(&format!("asset-{index}"))?;
    }
    private_fs::replace(
        &journal.stage().join(MANIFEST),
        &bytes,
        "staged skill receipt",
    )
    .map_err(io_error)?;
    checkpoint("manifest-written")?;
    if !matches(&journal.stage(), journal.after.as_ref())?
        || !matches(directory, journal.before.as_ref())?
    {
        return Err(conflict());
    }
    checkpoint("stage-verified")?;
    journal.phase = Phase::Ready;
    journal.write()?;
    checkpoint("ready")?;
    finish(&mut journal)
}

pub(super) fn remove(directory: &Path, expected: &str) -> Result<(), Error> {
    let mut journal = Journal {
        target: directory.into(),
        nonce: nonce().to_string(),
        phase: Phase::Ready,
        before: proof(directory)?,
        after: None,
    };
    if journal.before.as_ref().map(|p| p.revision.as_str()) != Some(expected) {
        return Err(Error::new("conflict", ()));
    }
    for path in [journal.path(), journal.stage(), journal.backup()] {
        if present(&path)? {
            return Err(conflict());
        }
    }
    journal.write()?;
    checkpoint("ready")?;
    finish(&mut journal)
}
