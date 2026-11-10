//! Background failures with no screen of their own: the host error Settings shows.
//!
//! Each failure carries its typed [`Source`]. One failing episode of a source is
//! one occurrence with one id: repeats before recovery keep the id, so a
//! dismissal holds until the source recovers and fails again. The next success
//! of the same kind resolves it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    /// Host diagnostic journal writes.
    Journal,
    /// Attempt and decision traces of reviews and publications.
    AttemptDiagnostics,
    /// Connectivity probes and their saved observations.
    Connectivity,
    /// Starting repository checks.
    Checks,
    /// Saving one repository's poll result.
    Poll { repository: String },
    /// Flushing saved progress.
    Progress,
    /// The scheduler's copy of saved state.
    Scheduler,
    /// One scheduler coordinator pass.
    Pass { pass: Pass },
    /// Saved-queue compaction.
    Compaction,
    /// AI dispatch and slot release.
    Capacity,
    /// Saving a review worker's result.
    Review,
    /// Recording a rate-limit gate.
    Backpressure,
    /// Publication worker infrastructure.
    Publication,
    /// Exit write worker infrastructure.
    ExitWrites,
    /// Conversation reply worker infrastructure.
    FollowUps,
    /// Provider action worker infrastructure.
    Actions,
    /// Notification delivery and its saved ledger.
    Notifications,
    /// Native notification registration at startup.
    NotificationHost,
    /// The local control endpoint at startup.
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pass {
    Maintenance,
    Resume,
    Capacity,
    Publication,
    ExitWrites,
    FollowUps,
    Actions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: u64,
    source: Source,
    message: String,
    dismissed: bool,
}

/// The newest undismissed occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub id: u64,
    pub message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    next: u64,
    entries: Vec<Entry>,
}

impl Board {
    /// Records a failure; returns its occurrence id. A source already failing
    /// keeps its occurrence (and any dismissal) and takes the latest message.
    pub fn raise(&mut self, source: Source, message: String) -> u64 {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.source == source) {
            entry.message = message;
            return entry.id;
        }
        self.next += 1;
        self.entries.push(Entry {
            id: self.next,
            source,
            message,
            dismissed: false,
        });
        self.next
    }

    /// The source succeeded: its occurrence ends.
    pub fn resolve(&mut self, source: &Source) {
        self.entries.retain(|e| &e.source != source);
    }

    /// Records `result` for `source`: a failure raises, a success resolves.
    pub fn settle(&mut self, source: Source, result: Result<(), String>) {
        match result {
            Ok(()) => self.resolve(&source),
            Err(message) => {
                self.raise(source, message);
            }
        }
    }

    /// Hides one occurrence. A later occurrence of the same source shows again.
    pub fn dismiss(&mut self, id: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.dismissed = true;
        }
    }

    pub fn current(&self) -> Option<Shown> {
        self.entries
            .iter()
            .filter(|e| !e.dismissed)
            .max_by_key(|e| e.id)
            .map(|e| Shown {
                id: e.id,
                message: e.message.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll(repository: &str) -> Source {
        Source::Poll {
            repository: repository.into(),
        }
    }

    #[test]
    fn a_dismissed_occurrence_stays_hidden_while_its_source_keeps_failing() {
        let mut board = Board::default();
        let id = board.raise(Source::Checks, "Storage is unavailable.".into());
        board.dismiss(id);
        assert_eq!(board.current(), None);
        assert_eq!(board.raise(Source::Checks, "Other failure.".into()), id);
        assert_eq!(board.current(), None);
    }

    #[test]
    fn a_new_occurrence_after_recovery_shows_again_with_a_new_id() {
        let mut board = Board::default();
        let first = board.raise(Source::Checks, "Storage is unavailable.".into());
        board.dismiss(first);
        board.settle(Source::Checks, Ok(()));
        board.settle(Source::Checks, Err("Storage is unavailable.".into()));
        let shown = board.current().expect("new occurrence");
        assert_ne!(shown.id, first);
        assert_eq!(shown.message, "Storage is unavailable.");
    }

    #[test]
    fn success_clears_only_its_own_source() {
        let mut board = Board::default();
        board.raise(poll("a"), "Poll a failed.".into());
        let journal = board.raise(Source::Journal, "Journal failed.".into());
        board.settle(poll("b"), Ok(()));
        board.settle(Source::Pass { pass: Pass::Resume }, Ok(()));
        assert_eq!(board.current().map(|s| s.id), Some(journal));
        board.settle(Source::Journal, Ok(()));
        assert_eq!(board.current().unwrap().message, "Poll a failed.");
        board.settle(poll("a"), Ok(()));
        assert_eq!(board.current(), None);
    }

    #[test]
    fn dismissing_one_occurrence_reveals_the_next_undismissed_one() {
        let mut board = Board::default();
        let older = board.raise(Source::Control, "Control failed.".into());
        let newer = board.raise(Source::Notifications, "Ledger failed.".into());
        board.dismiss(newer);
        assert_eq!(board.current().map(|s| s.id), Some(older));
        board.dismiss(older);
        assert_eq!(board.current(), None);
    }
}
