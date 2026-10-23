use crate::storage::Store;
use std::io::ErrorKind;

pub(crate) fn decode_queue(bytes: &[u8]) -> Result<crate::monitoring::QueueState, String> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum SavedQueue {
        Current(crate::monitoring::QueueState),
        Legacy(Vec<crate::monitoring::QueueJob>),
    }
    serde_json::from_slice::<SavedQueue>(bytes)
        .map(|saved| match saved {
            SavedQueue::Current(state) => state,
            SavedQueue::Legacy(jobs) => crate::monitoring::QueueState {
                jobs,
                ..Default::default()
            },
        })
        .map_err(|_| "Review queue is invalid; no polling result was saved.".into())
}

// Typed application state stays attached to Store without coupling settings,
// policy and filesystem tests to the native host and provider runtimes.
impl Store {
    pub fn load_actions(&self) -> Result<crate::actions::Ledger, String> {
        crate::retention::guard(self)?;
        self.decoded("actions.json", || match self.read_state("actions.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "Final review/action state is invalid; no provider action allowed.".into()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read final review/action state.".into()),
        })
    }
    pub fn save_actions(&self, ledger: &crate::actions::Ledger) -> Result<(), String> {
        crate::retention::reject_cleaned(self, ledger.finals.iter().map(|f| f.basis.job.clone()))?;
        crate::retention::reject_items(
            self,
            ledger
                .effects
                .iter()
                .map(|e| e.item_id.as_str())
                .chain(ledger.observations.iter().map(|o| o.item_id.as_str())),
        )?;
        self.commit_operational("actions.json", ledger)
    }
    pub fn load_feedback(&self) -> Result<crate::feedback::Ledger, String> {
        crate::retention::guard(self)?;
        let mut ledger: crate::feedback::Ledger =
            self.decoded("feedback.json", || match self.read_state("feedback.json") {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                    "Owned feedback state is invalid; no clearance may be inferred.".into()
                }),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
                Err(_) => Err("Cannot read owned feedback state.".into()),
            })?;
        if ledger.mentions.iter().any(|m| m.item_id.is_none()) {
            let queue = self.load_queue_state()?;
            let runs = self.load_follow_ups()?;
            let mut migrated = false;
            for mention in ledger.mentions.iter_mut().filter(|m| m.item_id.is_none()) {
                if let Ok(id) = mention.association(&queue.tracked, &queue.jobs, &runs) {
                    mention.item_id = Some(id);
                    migrated = true;
                }
            }
            // A read-only snapshot migrates in memory; the next real load saves.
            if migrated && !self.is_frozen() {
                // Pin proof before returning it, including index reads. Do not
                // recursively refresh the index that may have requested this load.
                crate::retention::mark_activity_pending(self)?;
                self.write_state("feedback.json", &ledger)?;
            }
        }
        Ok(ledger)
    }
    pub fn save_feedback(&self, ledger: &crate::feedback::Ledger) -> Result<(), String> {
        crate::retention::reject_cleaned(
            self,
            ledger
                .records
                .iter()
                .map(|r| r.job.clone())
                .chain(ledger.exit_writes.iter().map(|w| w.job.clone())),
        )?;
        for mention in &ledger.mentions {
            if crate::retention::known_key(self, &mention.key)? {
                return Err(
                    "A stale operation attempted to restore cleaned mention intent.".into(),
                );
            }
        }
        self.commit_operational("feedback.json", ledger)
    }
    pub fn load_pr_states(&self) -> Result<crate::pr_state::PrStates, String> {
        crate::retention::guard(self)?;
        self.decoded("pr-state.json", || match self.read_state("pr-state.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "PR state is invalid; no state change can be detected.".into()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read PR state. Check local file permissions.".into()),
        })
    }

    pub fn save_pr_states(&self, states: &crate::pr_state::PrStates) -> Result<(), String> {
        self.commit_operational("pr-state.json", states)
    }

    pub fn load_automation(&self) -> Result<crate::capacity::Automation, String> {
        match self.read_state("automation.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Automation state is invalid; repair it before running work.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read automation state. No work can start.".into()),
        }
    }

    pub fn save_automation(&self, automation: &crate::capacity::Automation) -> Result<(), String> {
        self.write_state("automation.json", automation)
    }

    pub fn load_backpressure(&self) -> Result<crate::backpressure::Backpressure, String> {
        match self.read_state("backpressure.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "Backpressure state is invalid; repair it before running work.".into()
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read backpressure state. No work can start.".into()),
        }
    }

    pub fn load_connectivity(&self) -> Result<crate::connectivity::Connectivity, String> {
        match self.read_state("connectivity.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "Connectivity state is invalid; repair it before running work.".into()
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read connectivity state. No work can start.".into()),
        }
    }

    pub fn save_connectivity(
        &self,
        state: &crate::connectivity::Connectivity,
    ) -> Result<(), String> {
        let offline = |c: &crate::connectivity::Connectivity| -> Vec<_> {
            c.services
                .iter()
                .map(|s| (s.scope.clone(), s.offline_since))
                .collect()
        };
        // Probe timestamps change every check; only reachability changes work.
        let changed = self
            .load_connectivity()
            .map_or(true, |previous| offline(&previous) != offline(state));
        // Not result evidence: no result-index rebuild for a liveness probe.
        crate::retention::guard(self)?;
        self.write_state("connectivity.json", state)?;
        if changed {
            self.wake_file("connectivity.json");
        }
        Ok(())
    }

    pub fn save_backpressure(
        &self,
        backpressure: &crate::backpressure::Backpressure,
    ) -> Result<(), String> {
        self.commit_operational("backpressure.json", backpressure)
    }

    pub fn load_queue(&self) -> Result<Vec<crate::monitoring::QueueJob>, String> {
        Ok(self.load_queue_state()?.jobs)
    }

    pub fn load_queue_state(&self) -> Result<crate::monitoring::QueueState, String> {
        crate::retention::guard(self)?;
        self.decoded("queue.json", || match self.read_state("queue.json") {
            Ok(bytes) => decode_queue(&bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read the review queue. Check local file permissions.".into()),
        })
    }

    pub fn save_queue(&self, jobs: &[crate::monitoring::QueueJob]) -> Result<(), String> {
        let mut state = self.load_queue_state()?;
        state.jobs = jobs.to_vec();
        self.save_queue_state(&state)
    }

    pub fn save_queue_state(&self, state: &crate::monitoring::QueueState) -> Result<(), String> {
        crate::retention::reject_cleaned(self, state.jobs.iter().cloned())?;
        let retention = crate::retention::load(self)?;
        if state.tracked.iter().any(|p| {
            p.lifecycle == crate::github::metadata::Lifecycle::Open
                && retention.receipts.iter().any(|r| {
                    crate::retention::Binding::tracked(&r.scope)
                        == crate::retention::Binding::tracked(p)
                        && p.iteration <= r.scope.iteration
                })
        }) {
            return Err(
                "A reopened PR requires a new iteration; retired work cannot be restored.".into(),
            );
        }
        // The currently committed tracking is legacy mention proof. Migrate
        // before replacing it; failure must leave that proof available to retry.
        self.load_feedback()?;
        self.commit_operational("queue.json", state)
    }

    pub fn allocate_enqueue_order(&self) -> Result<u64, String> {
        let mut state = self.load_queue_state()?;
        state.recover_evidence(self)?;
        let order = state.allocate_order()?;
        self.save_queue_state(&state)?;
        Ok(order)
    }

    pub fn load_notifications(&self) -> Result<crate::notifications::Ledger, String> {
        crate::retention::guard(self)?;
        self.decoded("notifications.json", || {
            let ledger = match self.read_state("notifications.json") {
                Ok(bytes) => serde_json::from_slice::<crate::notifications::Ledger>(&bytes)
                    .map_err(|_| "Notification history is invalid; no notices can be sent.")?,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    crate::notifications::Ledger::default()
                }
                Err(_) => {
                    return Err(
                        "Cannot read notification history. Check local storage permissions.".into(),
                    )
                }
            };
            ledger.validate()?;
            Ok(ledger)
        })
    }

    pub fn save_notifications(&self, ledger: &crate::notifications::Ledger) -> Result<(), String> {
        crate::retention::guard(self)?;
        // Notice IDs are receipt detail; their destinations stay fenced.
        crate::retention::reject_items(
            self,
            ledger.notices.iter().flat_map(|n| {
                let destination = match &n.event.destination {
                    crate::notifications::Destination::QueueItem { item_id } => {
                        Some(item_id.as_str())
                    }
                    _ => None,
                };
                std::iter::once(n.id.as_str()).chain(destination)
            }),
        )?;
        ledger.validate()?;
        self.write_state("notifications.json", ledger)
    }

    pub fn load_reviews(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
        crate::retention::guard(self)?;
        Ok(self.stored_reviews()?)
    }

    /// Saved reviews without the retention guard (cleanup recovery).
    pub(crate) fn stored_reviews(
        &self,
    ) -> Result<Vec<crate::review::ReviewRun>, crate::storage::review_store::ReadError> {
        Ok(self
            .review_runs()?
            .iter()
            .map(|run| run.as_ref().clone())
            .collect())
    }

    /// Replaces saved reviews without retention checks (cleanup recovery).
    /// A legacy file stays legacy: pruning an upgrade backlog shrinks it,
    /// and the next ordinary save migrates only what is left.
    pub(crate) fn replace_reviews(
        &self,
        reviews: &[crate::review::ReviewRun],
    ) -> Result<(), String> {
        if self.saved_is_legacy() {
            let bytes =
                serde_json::to_vec(reviews).map_err(|_| "Cannot encode monitoring state.")?;
            return self.write_legacy_reviews(&bytes);
        }
        self.write_reviews(reviews, None)
    }

    pub fn save_reviews(&self, reviews: &[crate::review::ReviewRun]) -> Result<(), String> {
        crate::retention::reject_cleaned(self, reviews.iter().map(|r| r.job.clone()))?;
        let fingerprint = review_transitions(reviews);
        if self.same_transition("reviews.json", fingerprint) {
            // Progress of a running review wakes no coordinator. Kept in
            // memory and written at most every `PROGRESS_FLUSH`.
            crate::retention::guard(self)?;
            if self.defer_progress(reviews, fingerprint) {
                return Ok(());
            }
            crate::retention::mark_activity_pending(self)?;
            return self.write_reviews(reviews, Some(fingerprint));
        }
        crate::retention::guard(self)?;
        crate::retention::mark_activity_pending(self)?;
        self.write_reviews(reviews, Some(fingerprint))?;
        crate::retention::refresh(self)
    }

    /// Publication records retain their immutable originating review even if an
    /// older queue/review file is absent. Reading evidence never rewrites history.
    pub fn review_evidence(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
        Ok(self.shared_evidence()?.as_ref().clone())
    }

    /// `review_evidence`, built once per version of its files and shared.
    pub(crate) fn shared_evidence(
        &self,
    ) -> Result<std::sync::Arc<Vec<crate::review::ReviewRun>>, String> {
        crate::retention::guard(self)?;
        self.cached(
            "review-evidence",
            &["reviews.json", "publications.json", "follow-ups.json"],
            || self.build_evidence(),
        )
    }

    fn build_evidence(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
        let reviews = self.load_reviews()?;
        let mut ids: std::collections::HashSet<_> =
            reviews.iter().map(|r| r.operation.id.clone()).collect();
        let mut evidence: Vec<_> = self
            .load_publications()?
            .into_iter()
            .map(|p| p.review)
            .chain(
                self.load_follow_ups()?
                    .into_iter()
                    .filter_map(|f| match f.target {
                        crate::follow_up::ConversationTarget::Owned(origin) => Some(origin.review),
                        _ => None,
                    }),
            )
            .filter(|r| ids.insert(r.operation.id.clone()))
            .collect();
        evidence.extend(reviews);
        Ok(evidence)
    }

    pub fn load_publications(&self) -> Result<Vec<crate::publication::Publication>, String> {
        crate::retention::guard(self)?;
        self.decoded("publications.json", || {
            match self.read_state("publications.json") {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                    "Publication state is invalid; no GitHub mutation is allowed.".into()
                }),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
                Err(_) => {
                    Err("Cannot read publication state. Check local storage permissions.".into())
                }
            }
        })
    }

    pub fn save_publications(
        &self,
        publications: &[crate::publication::Publication],
    ) -> Result<(), String> {
        crate::retention::reject_cleaned(self, publications.iter().map(|p| p.review.job.clone()))?;
        self.commit_operational("publications.json", publications)
    }

    pub fn load_follow_ups(&self) -> Result<Vec<crate::follow_up::FollowUp>, String> {
        crate::retention::guard(self)?;
        let decoded = self.cached(
            "follow-ups.json",
            &["follow-ups.json", "publications.json"],
            || match self.read_state("follow-ups.json") {
                Ok(bytes) => {
                    crate::follow_up::decode_with_origins(&bytes, &self.load_publications()?)
                        .map_err(|_| {
                            String::from(
                                "Thread follow-up state is invalid; automation is blocked.",
                            )
                        })
                }
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
                Err(_) => Err(
                    "Cannot read thread follow-up state. Check local storage permissions.".into(),
                ),
            },
        )?;
        Ok(decoded.as_ref().clone())
    }

    pub fn save_follow_ups(&self, runs: &[crate::follow_up::FollowUp]) -> Result<(), String> {
        crate::retention::reject_cleaned(self, runs.iter().map(|f| f.context.job.clone()))?;
        self.commit_operational("follow-ups.json", runs)
    }

    pub fn load_monitoring_state(&self) -> Result<crate::monitoring::MonitoringState, String> {
        crate::retention::guard(self)?;
        self.decoded("monitoring.json", || {
            match self.read_state("monitoring.json") {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|_| "Monitoring state is invalid; polling cannot resume.".into()),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    Ok(crate::monitoring::MonitoringState::default())
                }
                Err(_) => Err("Cannot read monitoring state. Check local file permissions.".into()),
            }
        })
    }

    pub fn save_monitoring_state(
        &self,
        state: &crate::monitoring::MonitoringState,
    ) -> Result<(), String> {
        crate::retention::guard(self)?;
        self.write_state("monitoring.json", state)
    }

    /// `load`'s value for the saved `name`, decoded once per version.
    fn decoded<T: Clone + Send + Sync + 'static>(
        &self,
        name: &'static str,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        Ok(self.cached(name, &[name], load)?.as_ref().clone())
    }

    fn commit_operational<T: serde::Serialize + ?Sized>(
        &self,
        name: &str,
        value: &T,
    ) -> Result<(), String> {
        crate::retention::guard(self)?;
        crate::retention::mark_activity_pending(self)?;
        self.write_state(name, value)?;
        crate::retention::refresh(self)
    }
}

/// Hash of the review facts coordinators act on: identity, job, operation
/// (state, attempts, deadlines, failure), error and whether context, packet
/// and result exist. Those three are set once, alongside an operation
/// change. Running progress (phase label, duck calls) is excluded.
fn review_transitions(reviews: &[crate::review::ReviewRun]) -> u64 {
    use std::hash::Hasher;
    struct Hashing(std::collections::hash_map::DefaultHasher);
    impl std::io::Write for Hashing {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.write(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hashing = Hashing(Default::default());
    for run in reviews {
        let _ = serde_json::to_writer(
            &mut hashing,
            &(
                run.feedback_context.is_some(),
                &run.key,
                &run.assignment_id,
                &run.job,
                &run.selection.agent.id,
                &run.operation,
                run.manual_start,
                run.trust_confirmed,
                &run.error,
                run.result.is_some(),
                &run.mode,
                run.packet.is_some(),
                &run.failed_duck,
            ),
        );
    }
    hashing.0.finish()
}
