//! Decoded saved state shared by a live Store and its frozen copies, so each
//! file is parsed once per change rather than on every read.
//!
//! Entries are keyed by `Stamp`s taken before the bytes are read: this
//! Store's write count plus the file's settled OS identity. A file without a
//! trustworthy stamp (changed within the racy window) is always decoded.

use super::file_identity::Identity;
use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// A saved file's version; the identity is `None` when the file is absent.
pub(crate) type Stamp = (u64, Option<Identity>);

type Entry = (Vec<Stamp>, Arc<dyn Any + Send + Sync>);

#[derive(Default)]
pub(crate) struct Cache {
    values: Mutex<HashMap<&'static str, Entry>>,
    pub(super) reviews: Mutex<super::review_store::Segments>,
    /// Live frozen copies by the review-store epoch they were taken at.
    readers: Mutex<BTreeMap<u64, usize>>,
    #[cfg(test)]
    parses: Mutex<BTreeMap<String, usize>>,
}

impl Cache {
    pub(super) fn get<T: Send + Sync + 'static>(
        &self,
        key: &'static str,
        stamps: &[Stamp],
    ) -> Option<Arc<T>> {
        let values = self.values.lock().ok()?;
        let (seen, value) = values.get(key)?;
        if seen.as_slice() != stamps {
            return None;
        }
        value.clone().downcast::<T>().ok()
    }

    pub(super) fn put<T: Send + Sync + 'static>(
        &self,
        key: &'static str,
        stamps: Vec<Stamp>,
        value: Arc<T>,
    ) {
        if let Ok(mut values) = self.values.lock() {
            values.insert(key, (stamps, value));
        }
    }

    pub(super) fn counted(&self, _key: &str) {
        #[cfg(test)]
        if let Ok(mut parses) = self.parses.lock() {
            *parses.entry(_key.to_string()).or_default() += 1;
        }
    }

    #[cfg(test)]
    pub(super) fn take_parses(&self) -> BTreeMap<String, usize> {
        std::mem::take(&mut *self.parses.lock().unwrap())
    }

    /// Registers a frozen copy taken at `epoch`; dropping the guard releases it.
    pub(super) fn reader(self: &Arc<Self>, epoch: u64) -> Arc<Reader> {
        if let Ok(mut readers) = self.readers.lock() {
            *readers.entry(epoch).or_default() += 1;
        }
        Arc::new(Reader {
            cache: Arc::downgrade(self),
            epoch,
        })
    }

    /// The oldest epoch a live frozen copy may still read from.
    pub(super) fn oldest_reader(&self) -> Option<u64> {
        self.readers
            .lock()
            .ok()
            .and_then(|readers| readers.keys().next().copied())
    }
}

/// Keeps review segments a frozen copy may still read on disk.
pub(crate) struct Reader {
    cache: std::sync::Weak<Cache>,
    epoch: u64,
}

impl Drop for Reader {
    fn drop(&mut self) {
        let Some(cache) = self.cache.upgrade() else {
            return;
        };
        let readers = cache.readers.lock();
        if let Ok(mut readers) = readers {
            if let Some(count) = readers.get_mut(&self.epoch) {
                *count -= 1;
                if *count == 0 {
                    readers.remove(&self.epoch);
                }
            }
        }
    }
}
