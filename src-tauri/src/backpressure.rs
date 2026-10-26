//! Rate limits are delay, not failure. A signal gates dispatch for the affected
//! provider account until the provider's reset, else a deterministic
//! exponential backoff. Gates persist in `backpressure.json`.

use crate::{monitoring::JobOperation, review::Failure, storage::Store};
use serde::{Deserialize, Serialize};

pub const BASE_SECONDS: i64 = crate::retry::BACKPRESSURE.base_seconds;
pub const CAP_SECONDS: i64 = crate::retry::BACKPRESSURE.cap_seconds;
// An expired gate nobody cleared is dropped after this idle time.
const STALE_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Github,
    Copilot,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Github { account_id: String },
    Copilot { account_id: String },
}

impl Scope {
    pub fn github(account_id: &str) -> Self {
        Self::Github {
            account_id: account_id.into(),
        }
    }

    pub fn copilot(account_id: &str) -> Self {
        Self::Copilot {
            account_id: account_id.into(),
        }
    }

    pub fn of(origin: Origin, github_account: &str, ai_account: Option<&str>) -> Self {
        match origin {
            Origin::Github => Self::github(github_account),
            Origin::Copilot => Self::copilot(ai_account.unwrap_or_default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub scope: Scope,
    pub until: i64,
    pub step: u8,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backpressure {
    #[serde(default)]
    pub gates: Vec<Gate>,
}

/// Seconds to wait: the provider's stated delay when present, else
/// `retry::BACKPRESSURE` full jitter seeded by `seed` and `step`.
pub fn delay_seconds(seed: &str, step: u8, retry_after_seconds: Option<i64>) -> i64 {
    crate::retry::BACKPRESSURE.wait(seed, u32::from(step), retry_after_seconds)
}

impl Backpressure {
    /// Records a signal and returns when the scope may be used again.
    pub fn signal(
        &mut self,
        scope: Scope,
        retry_after_seconds: Option<i64>,
        seed: &str,
        now: i64,
    ) -> i64 {
        self.gates
            .retain(|gate| gate.until.saturating_add(STALE_SECONDS) > now);
        let step = self
            .gates
            .iter()
            .find(|gate| gate.scope == scope)
            .map_or(0, |gate| gate.step.saturating_add(1));
        let until = now.saturating_add(delay_seconds(seed, step, retry_after_seconds));
        match self.gates.iter_mut().find(|gate| gate.scope == scope) {
            Some(gate) => {
                gate.until = gate.until.max(until);
                gate.step = step;
                gate.until
            }
            None => {
                self.gates.push(Gate { scope, until, step });
                until
            }
        }
    }

    /// A success clears the scope and its backoff history.
    pub fn note_success(&mut self, scope: &Scope) -> bool {
        let before = self.gates.len();
        self.gates.retain(|gate| &gate.scope != scope);
        self.gates.len() != before
    }

    pub fn until(&self, scope: &Scope, now: i64) -> Option<i64> {
        self.gates
            .iter()
            .find(|gate| &gate.scope == scope && gate.until > now)
            .map(|gate| gate.until)
    }

    pub fn until_any(&self, scopes: &[Scope], now: i64) -> Option<i64> {
        scopes.iter().filter_map(|s| self.until(s, now)).max()
    }
}

/// Persists a signal and returns the shared resume time.
pub fn record(
    store: &Store,
    scope: Scope,
    retry_after_seconds: Option<i64>,
    seed: &str,
    now: i64,
) -> Result<i64, String> {
    let mut state = store.load_backpressure()?;
    let until = state.signal(scope, retry_after_seconds, seed, now);
    store.save_backpressure(&state)?;
    Ok(until)
}

/// A call to the provider succeeded: its backoff history is forgotten.
pub fn succeeded(store: &Store, scope: &Scope) -> Result<(), String> {
    let mut state = store.load_backpressure()?;
    if state.note_success(scope) {
        store.save_backpressure(&state)?;
    }
    Ok(())
}

/// A whole AI run finished: both of its provider accounts proved usable.
pub fn run_succeeded(store: &Store, github_account: &str, ai_account: Option<&str>) {
    let mut state = match store.load_backpressure() {
        Ok(state) => state,
        Err(_) => return,
    };
    let mut changed = state.note_success(&Scope::github(github_account));
    if let Some(account) = ai_account {
        changed |= state.note_success(&Scope::copilot(account));
    }
    if changed {
        let _ = store.save_backpressure(&state);
    }
}

/// Turns a rate-limited failure into a delay on the operation and its scope.
/// Never counts an attempt or a failure. Returns whether the failure was one.
pub fn defer(
    store: &Store,
    operation: &mut JobOperation,
    error: Option<&Failure>,
    ai_account: Option<&str>,
    now: i64,
) -> Result<bool, String> {
    let Some(error) = error.filter(|e| e.rate_limited()) else {
        return Ok(false);
    };
    let scope = Scope::of(error.origin, &operation.account_id, ai_account);
    let until = record(store, scope, error.retry_after_seconds, &operation.id, now)?;
    operation.defer_until(until, now);
    Ok(true)
}

/// A rate-limited write that provably did not execute gates the account, and
/// its failure carries the shared resume time so the operation waits as long.
pub fn pace_write(
    store: &Store,
    failure: &mut Failure,
    uncertain: bool,
    account_id: &str,
    seed: &str,
    now: i64,
) {
    if uncertain || !failure.rate_limited() {
        return;
    }
    let scope = Scope::of(failure.origin, account_id, None);
    if let Ok(until) = record(store, scope, failure.retry_after_seconds, seed, now) {
        failure.retry_after_seconds = Some((until - now).max(1));
    }
}

#[cfg(test)]
mod tests;
