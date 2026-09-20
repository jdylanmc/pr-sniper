use pr_sniper_lib::storage::Store;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture(PathBuf);

impl Fixture {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "pr-sniper-storage-test-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated storage fixture");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn store(&self) -> Store {
        Store::new(self.0.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove this test's storage fixture");
    }
}
