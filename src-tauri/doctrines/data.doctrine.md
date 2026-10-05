---
name: data
description: "Review storage and processing changes for honest guarantees, safe replay, and observable recovery."
scope: default-pr-sniper-doctrine
---

# Data Doctrine

Data outlives operations. Acceptance, commit, reader visibility, external effects, and survival through failure are different promises. Durable computations will run again.

Review changed reads, writes, and pipelines under restart, lag, duplicates, mixed versions, and replay after surrounding state changes.

Look for:

- **Authority.** Identify the owning store, permitted writers, derived copies, and consumer freshness assumptions. Keep tightly consistent business data under one authority, not arbitrarily split services.
- **Guarantees.** Check restart durability, reader visibility, permitted staleness, conflict detection and resolution. Transactions need named invariants, atomic boundaries, unacceptable anomalies, and suitable isolation/concurrency control. Weaker guarantees need explicit compensation.
- **Workload fit.** Representation, storage, and indexes follow relationships, changes, queries, consistency, and evolution. Reshaping needs measured volume, read/write patterns, latency, throughput, contention, growth, and slow paths.
- **Derived state.** Every cache, index, projection, warehouse, or denormalized copy needs a source, update path, acceptable lag, drift visibility, and rebuild/repair strategy. Unrebuildable state is an authority.
- **Version coexistence.** Schemas, encodings, interfaces, messages, readers, writers, and retained records must tolerate rollout overlap.
- **Replay semantics.** Events record facts; commands request work; streams carry records; views derive state. Inputs, reference data, intermediate state, outputs, checkpoints, and sink acceptance need replay meaning. Checkpointing before durable effects loses work; afterward risks duplicates.
- **Time and order.** Name business-required ordering scope: record, entity, key, partition, stream, window, or history. Distinguish occurrence, arrival, and processing time for joins, windows, and late data. Retention bounds recovery and recomputation.
- **Visible recovery.** Expose backlog, watermarks, staleness, conflicting writes, migration/rebuild failures, repair paths, and incomplete results. Hidden inconsistency is not eventual consistency.
- **External effects.** Reprocessing must not silently repeat irreversible actions. Look for stable operation identity and compensation.

Ground findings in concrete loss, duplication, staleness, or recovery scenarios. Single-store guarantees do not establish cross-node coordination, retry identity, or safe migration order.
