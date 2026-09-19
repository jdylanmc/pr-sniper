---
name: data-processing
description: "Make batch and stream processing replayable, restartable, time-aware, and honest about lag."
scope: shared-engineering-doctrine
---

# Data Processing Doctrine

## Prime directive

Assume every durable computation will run again.

## Position

Batch jobs and stream processors live across time. They restart, fall behind, see duplicates, consume old and new records together, and revisit history after the world has changed. A pipeline that only works once, in order, at the present moment is not durable processing.

Replayability is not merely rerunning code. The same inputs, checkpoints, reference data, and external effects must have deliberate meaning when execution resumes or history is processed again.

## Principles

- **Name the unit of ordering.** Preserve order only where the business requires it, and state whether that scope is a record, entity, key, partition, stream, window, or whole history.
- **Separate facts from instructions and views.** Events record what happened. Commands request work. Streams carry records. Materialized views are derived copies. Each has different replay and ownership obligations.
- **Make restart a normal path.** Declare inputs, outputs, checkpoints, intermediate state, and the point at which a sink accepts work. A checkpoint that advances before its effects are durable creates invisible loss; one that advances afterward may create duplicates.
- **Distinguish time.** When something happened, when it arrived, and when it was processed are different facts. Windows, joins, and late arrivals must choose which time they mean.
- **Design for history.** Retention defines how far recovery and recomputation can reach. Processing must tolerate records written by older and newer versions while history remains mixed.
- **Make lag visible.** Falling behind is a system state, not a private implementation detail. Expose backlog, watermark, staleness, failed recovery, and the point beyond which results are incomplete.
- **Keep derived results rebuildable.** A projection, index, or aggregate needs a known source, replay path, and repair strategy. If it cannot be rebuilt, it is an authority and must be treated as one.
- **Protect effects outside the pipeline.** Reprocessing must not silently repeat irreversible actions. Stable operation identity and compensation belong to Idempotency doctrine; this doctrine ensures the processing path exposes where they are needed.

## Boundary

Data Processing owns computation across replay and time. Data owns storage, schema, and consistency contracts. Distributed Data owns cross-node coordination. Idempotency owns logical operation identity and recovery from duplicate effects.
