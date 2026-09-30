use crate::storage::Store;
use std::io::ErrorKind;

// Typed application state stays attached to Store without coupling settings,
// policy and filesystem tests to the native host and provider runtimes.
impl Store {
    pub fn load_queue(&self) -> Result<Vec<crate::monitoring::QueueJob>, String> {
        match self.read_state("queue.json") {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Review queue is invalid; no polling result was saved.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err("Cannot read the review queue. Check local file permissions.".into()),
        }
    }

    pub fn save_queue(&self, jobs: &[crate::monitoring::QueueJob]) -> Result<(), String> {
        self.write_state("queue.json", jobs)
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
