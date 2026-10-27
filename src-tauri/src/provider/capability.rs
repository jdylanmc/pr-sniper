//! Typed provider capabilities: which operations PR Sniper offers for each
//! provider, and why one is unavailable.

use crate::storage::ProviderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Available,
    Unavailable(Unavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// PR Sniper does not implement the operation for this provider yet.
    NotImplemented,
    /// PR Sniper never offers the operation for this provider.
    NotSupported,
}

impl Capability {
    pub fn is_available(self) -> bool {
        self == Self::Available
    }

    /// The first unavailable capability, else `Available`.
    pub fn all(capabilities: &[Capability]) -> Capability {
        capabilities
            .iter()
            .copied()
            .find(|capability| !capability.is_available())
            .unwrap_or(Self::Available)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Account, repository and pull request reads.
    pub read: Capability,
    /// Review thread read, reply and resolve.
    pub threads: Capability,
    /// Review publication with inline threads.
    pub publish: Capability,
    /// Merge readiness and check status.
    pub readiness: Capability,
    /// Approval vote on an exact head.
    pub approve: Capability,
    /// Approve and merge (complete) on an exact head.
    pub approve_and_merge: Capability,
}

impl ProviderCapabilities {
    pub fn of(provider: &ProviderId) -> Self {
        use Capability::{Available, Unavailable as No};
        match provider {
            ProviderId::Github => Self {
                read: Available,
                threads: Available,
                publish: Available,
                readiness: Available,
                approve: Available,
                approve_and_merge: Available,
            },
            ProviderId::AzureDevops => Self {
                read: No(Unavailable::NotImplemented),
                threads: No(Unavailable::NotImplemented),
                publish: No(Unavailable::NotImplemented),
                readiness: No(Unavailable::NotImplemented),
                approve: No(Unavailable::NotImplemented),
                approve_and_merge: No(Unavailable::NotSupported),
            },
        }
    }

    /// Everything monitoring and review need: reads, threads, publication and
    /// readiness.
    pub fn review(&self) -> Capability {
        Capability::all(&[self.read, self.threads, self.publish, self.readiness])
    }
}
