# Append conditions enforced as read-conflict-range probes inside the write transaction

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

An `AppendCondition` must fail the append if a matching event exists after `after` — including events committed concurrently by racing writers. How is this enforced atomically without locks?

## Decision Drivers

- Correctness under arbitrary concurrency (exactly one winner among racers)
- No application-level locking or coordination service
- Cheap in the common (non-conflicting) case

## Considered Options

- Existence probes inside the write transaction (FDB MVCC read-conflict ranges)
- App-level locks / lease per boundary
- Two-phase: read outside txn, then compare a version key inside

## Decision Outcome

Chosen option: "Probes inside the write transaction". Each condition's query items are translated to index ranges (thanks to ADR-0005) and probed with `limit=1` existence reads in the same transaction as the event writes (`append.rs:108-150`). If a probe finds a match → `AppendConditionFailed`. If it finds nothing, the *empty range read is still registered as a read-conflict range*: any concurrent commit inserting a matching event invalidates this transaction at commit, and FDB retries it — on retry the probe sees the new event and fails cleanly. All probes are issued concurrently and pipelined.

### Positive Consequences

- Serializable one-winner semantics for free (asserted by `tests/concurrency_tests.rs`: 10 racers, exactly one success)
- Failed conditions leave the store untouched (probes + writes share the txn; `append_conditions.rs` asserts atomicity)
- Retryable FDB errors during probes defer the verdict safely — retry re-checks every condition

### Negative Consequences

- Conflict granularity = index-range granularity: a condition conflicts with any write into its ranges, so broad queries serialize writers
- Correctness silently depends on the index layout (ADR-0005) covering the query exactly — the reason the Lean proof (ADR-0013) exists

## Pros and Cons of the Options

### In-transaction probes

- Good, because atomic, lock-free, and precise
- Bad, because subtle — relies on FDB registering empty reads as conflict ranges

### App-level locks

- Good, because easy to reason about
- Bad, because a second consistency mechanism with its own failure modes (lock leaks, fencing)

### Two-phase version compare

- Good, because simple
- Bad, because requires a materialized version key per boundary — but boundaries are dynamic queries, unbounded in number

## Links

- Implemented by `dcb-layer/src/append.rs:99-150,201-225` (`query_item_exists`)
- Verified by `dcb-layer/tests/concurrency_tests.rs`, `dcb-layer/tests/condition_semantics_tests.rs`
- Related: ADR-0005 (ranges), ADR-0013 (proof)
