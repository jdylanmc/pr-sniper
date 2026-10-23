//! Saved reviews: `state/reviews.json` is a small manifest of content-addressed
//! run segments under `state/reviews/`, so saving one run's progress writes
//! that run and the manifest instead of every review.
//!
//! - `run-<hash>.json`: one run, with its large repeated snapshots
//!   (`selection`, `feedback_context`) stored once as `part-<hash>.json`.
//! - Files are immutable: a changed run gets a new name. The manifest replace
//!   is the commit point; unreferenced files are collected after it, once no
//!   frozen copy that may still read them is alive.
//! - A legacy `reviews.json` array stays readable and becomes a manifest on
//!   the next save. Older builds refuse the manifest instead of reading it as
//!   empty.

use super::{private_fs, Store};
use crate::review::ReviewRun;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::ErrorKind;
use std::sync::Arc;

pub(crate) const MANIFEST: &str = "reviews.json";
const DIRECTORY: &str = "reviews";
const FORMAT: u32 = 2;
/// Run fields repeated verbatim across runs; each distinct value is saved once.
const PARTS: [&str; 2] = ["selection", "feedback_context"];

/// Why saved reviews could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadError {
    Unreadable,
    Invalid,
}

impl From<ReadError> for String {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::Unreadable => {
                "Cannot read review state. Check local storage permissions.".into()
            }
            ReadError::Invalid => "Review state is invalid; execution cannot resume.".into(),
        }
    }
}

use ReadError::{Invalid as INVALID, Unreadable as UNREADABLE};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    runs: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    run: Map<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    parts: BTreeMap<String, String>,
}

pub(super) enum Saved {
    Legacy(Vec<Arc<ReviewRun>>),
    Segmented(Vec<String>),
}

struct Run {
    value: Arc<ReviewRun>,
    parts: Vec<String>,
    /// Every instant-like value in the run and its parts (see `wake`).
    instants: Arc<Vec<i64>>,
}

struct Part {
    value: Arc<Value>,
    instants: Vec<i64>,
    /// The decoded field every run naming this part shares.
    typed: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

/// Points `run`'s field at the copy every run with the same `part` shares.
fn intern(run: &mut ReviewRun, field: &str, part: &mut Part) {
    match field {
        "selection" => match part.typed.clone().and_then(|t| t.downcast().ok()) {
            Some(shared) => run.selection = crate::review::Shared(shared),
            None => part.typed = Some(run.selection.0.clone()),
        },
        "feedback_context" => {
            if let Some(context) = &mut run.feedback_context {
                match part.typed.clone().and_then(|t| t.downcast().ok()) {
                    Some(shared) => *context = crate::review::Shared(shared),
                    None => part.typed = Some(context.0.clone()),
                }
            }
        }
        _ => {}
    }
}

/// Decoded segments by file name. Names are content hashes, so an entry is
/// valid for as long as the name exists.
#[derive(Default)]
pub(crate) struct Segments {
    runs: HashMap<String, Run>,
    parts: HashMap<String, Part>,
    /// Names unreferenced since the epoch they were recorded at.
    garbage: Vec<(u64, String, Vec<String>)>,
    swept: bool,
}

fn decode(bytes: &[u8]) -> Result<Saved, ReadError> {
    let first = bytes.iter().find(|b| !b.is_ascii_whitespace());
    if first == Some(&b'{') {
        let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
        if manifest.format != FORMAT || !manifest.runs.iter().all(|name| valid_name(name, "run-")) {
            return Err(INVALID);
        }
        return Ok(Saved::Segmented(manifest.runs));
    }
    let runs: Vec<ReviewRun> = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    Ok(Saved::Legacy(runs.into_iter().map(Arc::new).collect()))
}

fn valid_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|hash| hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn name(prefix: &str, bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut name = String::with_capacity(prefix.len() + 37);
    name.push_str(prefix);
    // 128 bits name a run or part uniquely and keep the manifest small.
    for byte in &digest[..16] {
        name.push_str(&format!("{byte:02x}"));
    }
    name.push_str(".json");
    name
}

fn instants(bytes: &[u8]) -> Vec<i64> {
    crate::wake::future_instants(bytes, i64::MIN)
}

impl Store {
    fn segment_directory(&self) -> std::path::PathBuf {
        self.root.join("state").join(DIRECTORY)
    }

    pub(super) fn saved_reviews(&self) -> Result<Arc<Option<Saved>>, ReadError> {
        self.cached(MANIFEST, &[MANIFEST], || match self.read_state(MANIFEST) {
            Ok(bytes) => decode(&bytes).map(Some),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(UNREADABLE),
        })
    }

    /// Every saved run, shared with the decode cache. No retention guard.
    pub(crate) fn review_runs(&self) -> Result<Vec<Arc<ReviewRun>>, ReadError> {
        match self.saved_reviews()?.as_ref() {
            None => Ok(Vec::new()),
            Some(Saved::Legacy(runs)) => Ok(runs.clone()),
            Some(Saved::Segmented(names)) => names
                .iter()
                .map(|name| self.run(name).map(|run| run.0))
                .collect(),
        }
    }

    /// `run` value and instants, decoding it (outside every lock) on a miss.
    fn run(&self, name: &str) -> Result<(Arc<ReviewRun>, Arc<Vec<i64>>), ReadError> {
        if let Some(run) = self
            .cache
            .reviews
            .lock()
            .map_err(|_| UNREADABLE)?
            .runs
            .get(name)
        {
            return Ok((run.value.clone(), run.instants.clone()));
        }
        self.cache.counted("reviews/run");
        let bytes = self.segment_bytes(name)?;
        let segment: Segment = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        let mut object = segment.run;
        let mut all = instants(&bytes);
        let mut parts = Vec::with_capacity(segment.parts.len());
        let mut fields = Vec::with_capacity(segment.parts.len());
        for (field, part) in segment.parts {
            if !PARTS.contains(&field.as_str()) || !valid_name(&part, "part-") {
                return Err(INVALID);
            }
            let (value, part_instants) = self.part(&part)?;
            all.extend(part_instants);
            object.insert(field.clone(), (*value).clone());
            fields.push(field);
            parts.push(part);
        }
        let mut value: ReviewRun =
            serde_json::from_value(Value::Object(object)).map_err(|_| INVALID)?;
        if let Ok(mut segments) = self.cache.reviews.lock() {
            for (field, part) in fields.iter().zip(&parts) {
                if let Some(part) = segments.parts.get_mut(part) {
                    intern(&mut value, field, part);
                }
            }
        }
        all.sort_unstable();
        all.dedup();
        let run = Run {
            value: Arc::new(value),
            parts,
            instants: Arc::new(all),
        };
        let found = (run.value.clone(), run.instants.clone());
        if let Ok(mut segments) = self.cache.reviews.lock() {
            segments.runs.insert(name.to_string(), run);
        }
        Ok(found)
    }

    fn part(&self, name: &str) -> Result<(Arc<Value>, Vec<i64>), ReadError> {
        if let Some(part) = self
            .cache
            .reviews
            .lock()
            .map_err(|_| UNREADABLE)?
            .parts
            .get(name)
        {
            return Ok((part.value.clone(), part.instants.clone()));
        }
        self.cache.counted("reviews/part");
        let bytes = self.segment_bytes(name)?;
        let value: Arc<Value> = Arc::new(serde_json::from_slice(&bytes).map_err(|_| INVALID)?);
        let found = (value.clone(), instants(&bytes));
        if let Ok(mut segments) = self.cache.reviews.lock() {
            segments.parts.insert(
                name.to_string(),
                Part {
                    value,
                    instants: found.1.clone(),
                    typed: None,
                },
            );
        }
        Ok(found)
    }

    fn segment_bytes(&self, name: &str) -> Result<Vec<u8>, ReadError> {
        let directory = self.segment_directory();
        private_fs::existing_directory(&self.root)
            .and_then(|()| private_fs::existing_directory(&directory))
            .and_then(|()| private_fs::read(&directory.join(name)))
            .map_err(|error| match error.kind() {
                ErrorKind::NotFound => INVALID,
                _ => UNREADABLE,
            })
    }

    /// Future instants saved in `name`, read as one document even when it is
    /// stored as segments.
    pub(crate) fn saved_instants(&self, name: &str, now: i64) -> Vec<i64> {
        if name == MANIFEST {
            if let Ok(saved) = self.saved_reviews() {
                if let Some(Saved::Segmented(names)) = saved.as_ref() {
                    let mut found: Vec<i64> = names
                        .iter()
                        .filter_map(|name| self.run(name).ok())
                        .flat_map(|(_, instants)| {
                            instants
                                .iter()
                                .copied()
                                .filter(|&at| at > now)
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    found.sort_unstable();
                    found.dedup();
                    return found;
                }
            }
        }
        self.read_state(name)
            .map(|bytes| crate::wake::future_instants(&bytes, now))
            .unwrap_or_default()
    }

    /// Saves `reviews` as the manifest, writing only runs that no saved
    /// segment already holds. `fingerprint` as in `write_state_as`.
    pub(crate) fn write_reviews(
        &self,
        reviews: &[ReviewRun],
        fingerprint: Option<u64>,
    ) -> Result<(), String> {
        if self.is_frozen() {
            return Err("A read-only state snapshot cannot be saved.".into());
        }
        let previous: Vec<(String, Arc<ReviewRun>)> = match self.saved_reviews() {
            Ok(saved) => match saved.as_ref() {
                Some(Saved::Segmented(names)) => names
                    .iter()
                    .filter_map(|name| self.run(name).ok().map(|run| (name.clone(), run.0)))
                    .collect(),
                _ => Vec::new(),
            },
            Err(_) => Vec::new(),
        };
        let mut by_id: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, (_, run)) in previous.iter().enumerate() {
            by_id
                .entry(run.operation.id.as_str())
                .or_default()
                .push(index);
        }
        let directory = self.segment_directory();
        let mut wrote = false;
        let mut created = false;
        let mut names = Vec::with_capacity(reviews.len());
        for run in reviews {
            let kept = by_id
                .get(run.operation.id.as_str())
                .and_then(|indexes| indexes.iter().find(|&&i| *previous[i].1 == *run));
            match kept {
                Some(&index) => names.push(previous[index].0.clone()),
                None => {
                    if !created {
                        private_fs::directory(&directory)
                            .map_err(|_| "Cannot create monitoring state directory.")?;
                        created = true;
                    }
                    let (name, written) = self.write_run(&directory, run)?;
                    wrote |= written;
                    names.push(name);
                }
            }
        }
        if wrote {
            private_fs::sync_entries(&directory).map_err(|_| "Cannot write monitoring state.")?;
        }
        // Compact: rewritten by every progress save.
        let manifest = serde_json::to_vec(&Manifest {
            format: FORMAT,
            runs: names.clone(),
        })
        .map_err(|_| "Cannot encode monitoring state.")?;
        self.write_state_bytes(MANIFEST, &manifest, fingerprint)?;
        let unreferenced: Vec<String> = previous
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| !names.contains(name))
            .collect();
        self.collect(&names, unreferenced);
        Ok(())
    }

    fn write_run(
        &self,
        directory: &std::path::Path,
        run: &ReviewRun,
    ) -> Result<(String, bool), String> {
        let encode = |_| "Cannot encode monitoring state.".to_string();
        let write = |_| "Cannot write monitoring state.".to_string();
        let Value::Object(mut object) = serde_json::to_value(run).map_err(encode)? else {
            return Err("Cannot encode monitoring state.".into());
        };
        let mut wrote = false;
        let mut parts = BTreeMap::new();
        let mut all = Vec::new();
        for field in PARTS {
            let Some(value) = object.remove(field) else {
                continue;
            };
            let bytes = serde_json::to_vec(&value).map_err(encode)?;
            let part = name("part-", &bytes);
            if private_fs::create_immutable(directory, &part, &bytes).map_err(write)? {
                wrote = true;
                self.record_bytes(bytes.len());
            }
            let part_instants = instants(&bytes);
            all.extend(part_instants.iter().copied());
            if let Ok(mut segments) = self.cache.reviews.lock() {
                let entry = segments.parts.entry(part.clone()).or_insert(Part {
                    value: Arc::new(value),
                    instants: part_instants,
                    typed: None,
                });
                if entry.typed.is_none() {
                    entry.typed = match field {
                        "selection" => Some(run.selection.0.clone()),
                        _ => run.feedback_context.as_ref().map(|context| {
                            context.0.clone() as Arc<dyn std::any::Any + Send + Sync>
                        }),
                    };
                }
            }
            parts.insert(field.to_string(), part);
        }
        let bytes = serde_json::to_vec(&Segment {
            run: object,
            parts: parts.clone(),
        })
        .map_err(encode)?;
        let segment = name("run-", &bytes);
        if private_fs::create_immutable(directory, &segment, &bytes).map_err(write)? {
            wrote = true;
            self.record_bytes(bytes.len());
        }
        all.extend(instants(&bytes));
        all.sort_unstable();
        all.dedup();
        if let Ok(mut segments) = self.cache.reviews.lock() {
            let mut value = run.clone();
            for (field, part) in &parts {
                if let Some(part) = segments.parts.get_mut(part) {
                    intern(&mut value, field, part);
                }
            }
            segments.runs.entry(segment.clone()).or_insert(Run {
                value: Arc::new(value),
                parts: parts.into_values().collect(),
                instants: Arc::new(all),
            });
        }
        Ok((segment, wrote))
    }

    /// Best-effort removal of segment files no manifest references, once no
    /// frozen copy that may read them is alive. A committed save never fails
    /// here; leftovers are retried by later saves.
    fn collect(&self, referenced: &[String], unreferenced: Vec<String>) {
        let oldest = self.cache.oldest_reader();
        let Ok(mut segments) = self.cache.reviews.lock() else {
            return;
        };
        let epoch = self
            .cache
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        for name in unreferenced {
            let parts = segments
                .runs
                .get(&name)
                .map(|run| run.parts.clone())
                .unwrap_or_default();
            segments.garbage.push((epoch, name, parts));
        }
        let mut keep: HashSet<String> = HashSet::new();
        for name in referenced {
            // Without a referenced run's parts, nothing can be proven unused.
            let Some(run) = segments.runs.get(name) else {
                return;
            };
            keep.insert(name.clone());
            keep.extend(run.parts.iter().cloned());
        }
        let directory = self.segment_directory();
        let removable = |since: u64| oldest.is_none_or(|oldest| oldest >= since);
        let garbage = std::mem::take(&mut segments.garbage);
        for (since, name, parts) in garbage {
            if keep.contains(&name) {
                continue;
            }
            if !removable(since) {
                segments.garbage.push((since, name, parts));
                continue;
            }
            for file in std::iter::once(&name).chain(parts.iter()) {
                if !keep.contains(file) {
                    segments.runs.remove(file);
                    segments.parts.remove(file);
                    let _ = std::fs::remove_file(directory.join(file));
                }
            }
        }
        if !segments.swept && oldest.is_none() {
            segments.swept = true;
            // Leftovers of earlier processes: anything this manifest does not
            // reference, including interrupted stages.
            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    let Some(name) = entry.file_name().to_str().map(String::from) else {
                        continue;
                    };
                    if !keep.contains(&name) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
            segments.garbage.clear();
        }
        segments.runs.retain(|name, _| keep.contains(name));
        segments.parts.retain(|name, _| keep.contains(name));
    }
}

#[cfg(test)]
#[path = "review_store_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) fn segment_files(store: &Store) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(store.segment_directory())
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}
