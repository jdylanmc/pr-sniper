use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const OPERATION_LIMIT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct AccountWork {
    pub gate: Arc<tokio::sync::Mutex<()>>,
    generation: Mutex<u64>,
}

impl AccountWork {
    pub fn invalidate<T>(&self, change: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let mut generation = self
            .generation
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?;
        *generation = generation.wrapping_add(1);
        change()
    }
}

#[derive(Clone)]
pub struct Operation {
    pub cancelled: Arc<AtomicBool>,
    pub deadline: Instant,
    pub account: Arc<AccountWork>,
    generation: u64,
    quitting: Arc<AtomicBool>,
}

impl Operation {
    pub fn new(
        account: Arc<AccountWork>,
        quitting: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Self, String> {
        let generation = *account
            .generation
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?;
        Ok(Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline,
            account,
            generation,
            quitting,
        })
    }

    fn check_current(&self, generation: u64) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst)
            || self.quitting.load(Ordering::SeqCst)
            || generation != self.generation
        {
            return Err("Copilot operation cancelled or account connection changed.".into());
        }
        Ok(())
    }

    fn check_generation(&self, generation: u64) -> Result<(), String> {
        self.check_current(generation)?;
        if Instant::now() >= self.deadline {
            return Err("Copilot operation timed out. Retry.".into());
        }
        Ok(())
    }

    pub fn check(&self) -> Result<(), String> {
        self.check_generation(
            *self
                .account
                .generation
                .lock()
                .map_err(|_| "Copilot account state is unavailable.")?,
        )
    }

    pub fn publish<T>(&self, change: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let generation = self
            .account
            .generation
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?;
        self.check_generation(*generation)?;
        change()
    }

    pub fn complete<T>(&self, change: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let generation = self
            .account
            .generation
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?;
        self.check_current(*generation)?;
        change()
    }

    pub fn finish_transaction<T>(
        &self,
        change: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let generation = self
            .account
            .generation
            .lock()
            .map_err(|_| "Copilot account state is unavailable.")?;
        // Cancellation/deadline cannot hide a completed credential transaction's
        // failure, but it must never change a replacement connection.
        if *generation != self.generation {
            return Err("Copilot account connection changed.".into());
        }
        change()
    }

    async fn stopped(&self) -> String {
        loop {
            if let Err(error) = self.check() {
                return error;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn wait<T>(&self, work: impl Future<Output = T>) -> Result<T, String> {
        self.check()?;
        tokio::select! {
            biased;
            reason = self.stopped() => Err(reason),
            _ = tokio::time::sleep_until(self.deadline.into()) => Err("Copilot operation timed out. Retry.".into()),
            value = work => {
                self.check()?;
                Ok(value)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation() -> Operation {
        Operation::new(
            Arc::new(AccountWork::default()),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + OPERATION_LIMIT,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn invalidated_generation_cannot_publish_complete_or_finish_a_transaction() {
        let operation = operation();
        let _gate = operation.account.gate.lock().await;
        operation.account.invalidate(|| Ok(())).unwrap();
        assert!(operation.check().is_err());
        assert!(operation.publish::<()>(|| panic!("stale publish")).is_err());
        assert!(operation
            .complete::<()>(|| panic!("stale completion"))
            .is_err());
        assert!(operation
            .finish_transaction::<()>(|| panic!("stale transaction"))
            .is_err());
        assert!(operation
            .wait(async { panic!("stale work") })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn cancellation_and_quit_bound_pending_work_without_hiding_transaction_failures() {
        for quitting in [false, true] {
            let operation = operation();
            let trigger = async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if quitting {
                    operation.quitting.store(true, Ordering::SeqCst);
                } else {
                    operation.cancelled.store(true, Ordering::SeqCst);
                }
            };
            let (result, ()) = tokio::join!(operation.wait(std::future::pending::<()>()), trigger);
            assert!(result.unwrap_err().contains("cancelled"));
            assert_eq!(
                operation.finish_transaction(|| Err::<(), _>("storage failure".into())),
                Err("storage failure".into())
            );
        }
    }

    #[tokio::test]
    async fn deadline_limits_pending_work_and_does_not_change_another_account() {
        let mut expired = operation();
        expired.deadline = Instant::now() + Duration::from_millis(20);
        let current = operation();
        assert!(expired
            .wait(std::future::pending::<()>())
            .await
            .unwrap_err()
            .contains("timed out"));
        assert_eq!(current.publish(|| Ok(42)).unwrap(), 42);
        assert_eq!(current.complete(|| Ok(43)).unwrap(), 43);
        assert_eq!(current.wait(async { 44 }).await.unwrap(), 44);
    }
}
