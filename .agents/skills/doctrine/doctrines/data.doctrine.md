---
name: data
description: "Make stored data authoritative, durable, visible, evolvable, and shaped by real access."
scope: shared-engineering-doctrine
---

# Data Doctrine

## Prime directive

State what owns the truth and what every read and write is allowed to promise.

## Position

Stored data outlives the operation that produced it. A design must distinguish request acceptance, committed state, reader visibility, external effects, and survival through failure. Treating them as one event creates guarantees the system never actually made.

Storage choices follow the data’s relationships, access patterns, consistency needs, evolution pressure, and measured workload—not fashion or a diagram drawn before the evidence.

## Principles

- **Name the authority.** Identify the owning store, who may change it, which copies are derived, and what each consumer may assume about freshness and consistency.
- **Define durability and visibility.** State when a write survives restart, when readers can observe it, whether stale reads are allowed, and how conflicts are detected and settled.
- **Measure before reshaping.** Understand volume, read and write patterns, latency, throughput, contention, growth, and the slowest important paths before changing engines, indexes, or layouts.
- **Fit representation to access.** Model relationships and fields according to how they change and are queried. Select storage and indexes according to observed use and required guarantees.
- **Own every second copy.** Caches, indexes, projections, search data, warehouses, and denormalized fields need a source, update path, acceptable lag, visibility into drift, and a way to rebuild or repair them.
- **Evolve contracts while versions coexist.** Schemas, encodings, interfaces, messages, and stored records must survive old and new readers, writers, and data during rollout.
- **Use transactions for named invariants.** Define what commits atomically, which anomalies are unacceptable, and which isolation or concurrency control protects the invariant. Weaker guarantees require an explicit compensating mechanism.
- **Draw service boundaries around ownership.** Keep data that must remain tightly consistent under one authority. Avoid splitting a business concept merely to create another service.
- **Make operational state visible.** Expose stale copies, failed rebuilds, conflicting writes, migration state, and repair paths. Hidden inconsistency is not eventual consistency.

## Boundary

Data owns storage and single-store consistency. Data Processing owns replay and computation through time. Distributed Data owns coordination across nodes and stores. Idempotency owns logical retries and ambiguous effects. Domain owns business meaning; Sequencing owns migration order.
