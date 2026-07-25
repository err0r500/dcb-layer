# Consistency model: per-append query conditions (DCB) instead of aggregate streams

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Classic event sourcing partitions events into streams (one per aggregate) and guards writes with an expected stream version. That makes cross-aggregate invariants awkward (sagas, reservation patterns). How should this store define its consistency boundary?

## Decision Drivers

- Invariants often span what would be several aggregates (e.g. uniqueness across entities)
- Avoid committing to an aggregate partitioning up front; let it evolve per use case
- Follow the published DCB specification (https://dcb.events) for interoperability of concepts

## Considered Options

- DCB: `AppendCondition { query, after }` evaluated per append
- Aggregate streams with expected-version checks
- Global serialized writer (single sequence, app-level validation)

## Decision Outcome

Chosen option: "DCB per-append conditions", because the consistency boundary becomes dynamic — each write declares the query it must be consistent with (`fail if any event matching `query` exists after `after``). Events carry free-form `tags` instead of a stream id, so one event can participate in many boundaries.

### Positive Consequences

- Multi-entity invariants in one atomic append, no process managers for simple cases
- The read-then-append pattern (`read → last position → append with after`) is the entire concurrency protocol
- Semantics are spec'd by tests (`condition_semantics_tests.rs`) and formally modeled (ADR-0013)

### Negative Consequences

- Conflict granularity is defined by the query: overly broad conditions serialize unrelated writers
- No per-stream contiguous version numbers; consumers must use global positions

## Pros and Cons of the Options

### DCB conditions

- Good, because boundary chosen per write, not per schema
- Bad, because misuse (broad queries) creates contention that stream-per-aggregate would have avoided

### Aggregate streams

- Good, because familiar, cheap conflict check (one version compare)
- Bad, because cross-aggregate invariants need sagas/reservations; boundaries frozen at design time

### Global serialized writer

- Good, because trivially correct
- Bad, because throughput collapses to one writer

## Links

- Implemented by `dcb-layer/src/types.rs` (`AppendCondition`, `Query`, `QueryItem`), `dcb-layer/src/append.rs`
- Specified by `dcb-layer/tests/condition_semantics_tests.rs`
- Related: ADR-0006 (enforcement mechanism), ADR-0013 (proof)
