---
name: distributed-data
description: "Make the costs and failure modes of data shared across machines explicit."
scope: shared-engineering-doctrine
---

# Distributed Data Doctrine

## Prime directive

Distribution turns assumptions into failure modes. Name the guarantees and pay their costs deliberately.

## Position

A remote value is not a local variable. Messages arrive late or twice. Replicas disagree. Processes pause. Clocks drift. Owners lose authority while still running. Networks divide systems precisely when coordination matters most.

Distributed design begins by admitting these conditions. Availability, latency, consistency, durability, and agreement cannot all be maximized at once. The system must state which guarantees matter for each operation and what callers observe when those guarantees cannot be met.

## Principles

- **Name replica visibility.** State whether reads may be stale, move backward, see their own writes, or require a consistent prefix. Expose lag and define how divergent copies converge.
- **Partition for real access.** Choose boundaries from locality, consistency, and workload. Account for hot keys, skew, routing, secondary indexes, rebalancing, and operations that must cross partitions.
- **Treat failover as a state transition.** Define who may become authoritative, what happens to in-flight work, and how clients distinguish retryable failure from unknown completion.
- **Fence stale owners.** Expiration alone does not stop an old leader from writing. Leases, ownership epochs, and fencing must make superseded authority unable to corrupt current state.
- **State the fault model.** Design for delayed and dropped messages, partitions, duplicate delivery, process pauses, clock uncertainty, and conflicting writers according to the environment that actually exists.
- **Use coordination only for named invariants.** Locks, quorum, consensus, ordered broadcast, and atomic commitment impose availability and latency costs. Buy them only when participants truly must agree.
- **Make cross-boundary atomicity honest.** A transaction inside one store does not make several stores or external effects atomic. Define partial outcomes, reconciliation, compensation, or explicit refusal.
- **Keep failure visible.** Replica lag, lost leadership, blocked quorum, repair, and convergence are operational states. A distributed system that hides them cannot be trusted when degraded.

## Boundary

Distributed Data owns guarantees across nodes, stores, and partitions. Data owns storage and single-store transaction semantics. Data Processing owns replay over time. Idempotency owns logical operation identity and duplicate-effect recovery. Domain and Boundaries own business meaning and ownership seams.
