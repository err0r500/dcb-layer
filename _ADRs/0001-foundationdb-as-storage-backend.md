# Event store persistence with FoundationDB as the storage backend

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

A DCB-compliant event store needs atomic multi-key appends, a globally ordered position for every event, and a way to enforce per-append consistency conditions under concurrency. Which storage backend should provide these primitives?

## Decision Drivers

- Atomic multi-key writes (event + up to 1024 index keys in one commit)
- A global, monotonic ordering primitive that does not require a serialized counter
- Serializable isolation strong enough to implement optimistic conditions without app-level locks
- Horizontal scalability and operational maturity

## Considered Options

- FoundationDB
- PostgreSQL (single table + advisory locks or serializable txns)
- Specialized event stores (Kurrentdb, AxonServer, UmaDB)

## Decision Outcome

Chosen option: "FoundationDB", because its versionstamps give commit-time global ordering for free,
its serializable MVCC transactions implement the DCB condition check as read-conflict ranges (no locks),
and multi-key atomic commits make the event + powerset index writes trivially consistent.

### Positive Consequences

- Ordering, atomicity, and optimistic concurrency all come from one engine (ADR-0004, ADR-0006)
- Ordered key space enables the range-scan index design (ADR-0005)
- One cluster serves many apps via namespacing (ADR-0011)

### Negative Consequences

- Hard 10 MB / 5 s transaction limits shape the whole API (batch caps, pagination — ADR-0010)
- Operational dependency on an FDB cluster; client requires `unsafe { boot() }` and a C library (`libfdb_c`)
- Smaller ecosystem than Postgres; bindgen-based client complicates cross-compilation (ADR-0014)
