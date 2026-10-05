---
name: data
description: "Make stored data and durable computation authoritative, replayable, visible, and shaped by real access."
scope: default-pr-sniper-doctrine
---

# Data Doctrine

Prime directive: state what owns the truth and what every read, write, and replay may promise. Assume every durable computation will run again.

Stored data outlives its operation. Separate request acceptance, committed state, reader visibility, external effects, and survival through failure. One event is not all of them.

Batch and stream work restarts, lags, duplicates, mixes old and new records, and revisits changed history. Once, in order, now, is not durable. Replay is not merely rerunning code. Inputs, checkpoints, reference data, and external effects need deliberate meaning on resume and on history.

Choose from relationships, access, consistency, evolution pressure, and measured workload. Not fashion. Not a diagram before evidence.

- Name the owning store, who may change it, derived copies, and each consumer's freshness and consistency assumptions.
- State restart survival, when readers observe a write, whether stale reads are allowed, and how conflicts are detected and settled.
- Measure volume, patterns, latency, throughput, contention, growth, and slowest paths before changing engines, indexes, or layouts.
- Model fields and relationships by change and query. Choose storage and indexes from observed use and required guarantees.
- Own every second copy. Caches, indexes, projections, search data, warehouses, and denormalized fields need a source, update path, lag, drift visibility, and rebuild or repair.
- Schemas, encodings, interfaces, messages, and stored records must survive old and new readers, writers, and data during rollout.
- Name each atomic commit, its unacceptable anomalies, and the isolation or concurrency control that protects it. Weaker guarantees need explicit compensation.
- Keep tightly consistent data under one authority. Do not split a business concept merely to create another service.
- Expose stale copies, failed rebuilds, conflicting writes, migration state, and repair paths. Hidden inconsistency is not eventual consistency.
- Preserve order only where the business requires it. Name the scope: record, entity, key, partition, stream, window, or whole history.
- Separate facts, instructions, and views. Events record what happened. Commands request work. Streams carry records. Materialized views are derived. Replay and ownership differ.
- Declare inputs, outputs, checkpoints, intermediate state, and sink acceptance. A checkpoint before durable effects hides loss. A later checkpoint may duplicate work.
- Occurrence, arrival, and processing time differ. Windows, joins, and late arrivals must name which time they mean.
- Retention bounds recovery and recomputation. Tolerate older and newer versions while history stays mixed.
- Lag is system state. Expose backlog, watermark, staleness, failed recovery, and where results become incomplete.
- A projection, index, or aggregate needs a source, replay path, and repair. If it cannot be rebuilt, it is an authority.
- Do not silently repeat irreversible actions. Effects leaving the pipeline need stable operation identity and compensation. Expose that need. Do not define operation identity here.

Owns storage, single-store consistency, and replay through time. Does not own cross-node coordination or full logical-retry identity. Name those limits. One store contract does not cover them. Bounded Context owns business meaning. Migration order is not a storage guarantee.
