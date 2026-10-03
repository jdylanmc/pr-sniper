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
        match self.read_state("actions.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "Final review/action state is invalid; no provider action allowed.".into()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read final review/action state.".into()),
        }
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
        match self.read_state("feedback.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "Owned feedback state is invalid; no clearance may be inferred.".into()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read owned feedback state.".into()),
        }
    }
    pub fn save_feedback(&self, ledger: &crate::feedback::Ledger) -> Result<(), String> {
        crate::retention::reject_cleaned(self, ledger.records.iter().map(|r| r.job.clone()))?;
        for mention in &ledger.mentions {
            if crate::retention::known_key(self, &mention.key)? {
                return Err(
                    "A stale operation attempted to restore cleaned mention intent.".into(),
                );
            }
        }
        self.commit_operational("feedback.json", ledger)
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

    pub fn load_queue(&self) -> Result<Vec<crate::monitoring::QueueJob>, String> {
        Ok(self.load_queue_state()?.jobs)
    }

    pub fn load_queue_state(&self) -> Result<crate::monitoring::QueueState, String> {
        crate::retention::guard(self)?;
        match self.read_state("queue.json") {
            Ok(bytes) => decode_queue(&bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(_) => Err("Cannot read the review queue. Check local file permissions.".into()),
        }
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
    }

    pub fn save_notifications(&self, ledger: &crate::notifications::Ledger) -> Result<(), String> {
        crate::retention::guard(self)?;
        crate::retention::reject_items(self, ledger.notices.iter().map(|n| n.id.as_str()))?;
        ledger.validate()?;
        self.write_state("notifications.json", ledger)
    }

    pub fn load_reviews(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
        crate::retention::guard(self)?;
        match self.read_state("reviews.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Review state is invalid; execution cannot resume.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err("Cannot read review state. Check local storage permissions.".into()),
        }
    }

    pub fn save_reviews(&self, reviews: &[crate::review::ReviewRun]) -> Result<(), String> {
        crate::retention::reject_cleaned(self, reviews.iter().map(|r| r.job.clone()))?;
        self.commit_operational("reviews.json", reviews)
    }

    /// Publication records retain their immutable originating review even if an
    /// older queue/review file is absent. Reading evidence never rewrites history.
    pub fn review_evidence(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
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
        match self.read_state("publications.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Publication state is invalid; no GitHub mutation is allowed.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err("Cannot read publication state. Check local storage permissions.".into()),
        }
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
        match self.read_state("follow-ups.json") {
            Ok(bytes) => crate::follow_up::decode_with_origins(&bytes, &self.load_publications()?)
                .map_err(|_| "Thread follow-up state is invalid; automation is blocked.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => {
                Err("Cannot read thread follow-up state. Check local storage permissions.".into())
            }
        }
    }

    pub fn save_follow_ups(&self, runs: &[crate::follow_up::FollowUp]) -> Result<(), String> {
        crate::retention::reject_cleaned(self, runs.iter().map(|f| f.context.job.clone()))?;
        self.commit_operational("follow-ups.json", runs)
    }

    pub fn load_monitoring_state(&self) -> Result<crate::monitoring::MonitoringState, String> {
        crate::retention::guard(self)?;
        match self.read_state("monitoring.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Monitoring state is invalid; polling cannot resume.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(crate::monitoring::MonitoringState::default())
            }
            Err(_) => Err("Cannot read monitoring state. Check local file permissions.".into()),
        }
    }

    pub fn save_monitoring_state(
        &self,
        state: &crate::monitoring::MonitoringState,
    ) -> Result<(), String> {
        crate::retention::guard(self)?;
        self.write_state("monitoring.json", state)
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
