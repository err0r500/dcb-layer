# Storage backend abstraction (store trait + in-memory implementation)?

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

`FdbStore` is a concrete type; every test needs Docker + a real FDB (ADR-0012), and downstream libraries (the planned "amzer" framework) can only be tested against FDB too. Would a `DcbStore` trait with an in-memory implementation pay for itself?

## Decision Drivers

- Fast, hermetic unit tests for downstream consumers
- Embedded/demo use without an FDB cluster
- Risk: the design is FDB-shaped — versionstamps, conflict ranges, watch semantics all leak

## Considered Options

- Trait + in-memory implementation in the core crate
- In-memory implementation in a separate crate, semantics-tested against the same suite
- No abstraction: FDB is the product (status quo)

## Decision Outcome

Leaning: "No abstraction in the core" — with a nuance. The public API (`append`, `read`, `read_all`, cursors, watch) is already backend-neutral in *shape*; what's FDB-specific is the guarantees' implementation, not their statement (append is atomic, positions are 12 opaque ordered bytes, conditions are serializable). If downstream test pain materializes, the right move is a separate `dcb-layer-mem` crate that passes the same behavioral test suite (extracted from `tests/`) — an in-memory single-mutex store can honor DCB semantics trivially. Do not abstract preemptively inside the core: a trait would freeze the API before a second real backend exists.

### Positive Consequences (of status quo + optional mem crate later)

- Core stays simple; no trait-object/async-trait tax on the hot path
- Behavioral test suite as the contract is stronger than a trait signature anyway

### Negative Consequences

- Downstream tests remain Docker-bound until/unless the mem crate exists
- The Elixir layer inherits the same constraint (testcontainers in `dcb-layer-ex` tests)

## Links

- `dcb-layer/src/types.rs` (`FdbStore`), `dcb-layer/tests/`
- `_notes/fw-ex-design.md` ("amzer" framework — the likely first consumer to want this)
- Related: ADR-0012
