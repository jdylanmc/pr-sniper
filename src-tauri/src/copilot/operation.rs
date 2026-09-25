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
