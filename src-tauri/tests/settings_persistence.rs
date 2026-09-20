use pr_sniper_lib::storage::{Settings, Store};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "pr-sniper-settings-test-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated settings fixture");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn store(&self) -> Store {
        Store::new(self.0.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove this test's settings fixture");
    }
}

#[test]
fn explicit_login_preference_survives_a_fresh_store() {
    let fixture = Fixture::new();
    let store = fixture.store();

    store
        .save_settings(&Settings {
            launch_at_login: true,
        })
        .expect("persist explicit opt-in in fixture, not the OS login items");
    drop(store);

    let reopened = Store::new(fixture.path().to_path_buf());
    assert!(
        reopened
            .load_settings()
            .expect("read persisted settings through a fresh Store")
            .launch_at_login,
        "an explicitly saved login preference must survive reopening storage"
    );
}
