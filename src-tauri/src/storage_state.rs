use crate::storage::Store;
use std::io::ErrorKind;

// Typed application state stays attached to Store without coupling settings,
// policy and filesystem tests to the native host and provider runtimes.
impl Store {
    pub fn load_queue(&self) -> Result<Vec<crate::monitoring::QueueJob>, String> {
        Ok(self.load_queue_state()?.jobs)
    }

    pub fn load_queue_state(&self) -> Result<crate::monitoring::QueueState, String> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum SavedQueue {
            Current(crate::monitoring::QueueState),
            Legacy(Vec<crate::monitoring::QueueJob>),
        }
        match self.read_state("queue.json") {
            Ok(bytes) => serde_json::from_slice::<SavedQueue>(&bytes)
                .map(|saved| match saved {
                    SavedQueue::Current(state) => state,
                    SavedQueue::Legacy(jobs) => crate::monitoring::QueueState {
                        jobs,
                        ..Default::default()
                    },
                })
                .map_err(|_| "Review queue is invalid; no polling result was saved.".into()),
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
        self.write_state("queue.json", state)
    }

    pub fn allocate_enqueue_order(&self) -> Result<u64, String> {
        let mut state = self.load_queue_state()?;
        state.recover_evidence(self)?;
        let order = state.allocate_order()?;
        self.save_queue_state(&state)?;
        Ok(order)
    }

    pub fn load_notifications(&self) -> Result<crate::notifications::Ledger, String> {
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
        ledger.validate()?;
        self.write_state("notifications.json", ledger)
    }

    pub fn load_reviews(&self) -> Result<Vec<crate::review::ReviewRun>, String> {
        match self.read_state("reviews.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Review state is invalid; execution cannot resume.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err("Cannot read review state. Check local storage permissions.".into()),
        }
    }

    pub fn save_reviews(&self, reviews: &[crate::review::ReviewRun]) -> Result<(), String> {
        self.write_state("reviews.json", reviews)
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
            .chain(self.load_follow_ups()?.into_iter().map(|f| f.review))
            .filter(|r| ids.insert(r.operation.id.clone()))
            .collect();
        evidence.extend(reviews);
        Ok(evidence)
    }

    pub fn load_publications(&self) -> Result<Vec<crate::publication::Publication>, String> {
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
        self.write_state("publications.json", publications)
    }

    pub fn load_follow_ups(&self) -> Result<Vec<crate::follow_up::FollowUp>, String> {
        match self.read_state("follow-ups.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Thread follow-up state is invalid; automation is blocked.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => {
                Err("Cannot read thread follow-up state. Check local storage permissions.".into())
            }
        }
    }

    pub fn save_follow_ups(&self, runs: &[crate::follow_up::FollowUp]) -> Result<(), String> {
        self.write_state("follow-ups.json", runs)
    }

    pub fn load_monitoring_state(&self) -> Result<crate::monitoring::MonitoringState, String> {
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
        self.write_state("monitoring.json", state)
    }
}
