# Cursor concurrency: last-writer-wins vs compare-and-swap / leases for consumer groups

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

`set_cursor` is last-writer-wins; the docs say "run one consumer per cursor name or coordinate externally" (`lib.rs:154`). The planned Elixir framework ("amzer", `_notes/fw-ex-design.md`) will run projections and automations under OTP supervision — where two instances of a consumer can briefly coexist during restarts/deploys, silently rewinding or skipping progress under LWW. Should the store offer stronger cursor primitives?

## Decision Drivers

- Supervisor restarts and rolling deploys make duplicate consumers a normal event, not an error
- FDB transactions make CAS trivial to implement server-side
- Keep the core minimal — coordination could also live in the framework layer

## Considered Options

- CAS cursor: `set_cursor_if(name, expected, new)` — fails on mismatch
- Lease-based ownership: consumer holds a fenced lease key; cursor writes require the fence token
- Keep LWW; solve duplicates in the framework (e.g. global process registry)

## Decision Outcome

Proposed: add "CAS cursor" as an optional primitive alongside LWW. It is a few lines in `subscribe.rs` (read + conditional set in one transaction — FDB gives the atomicity), gives frameworks a fencing building block without imposing leases, and is backward compatible. Full lease management stays out of the core; OTP-level singleton guarantees (registry/global) remain the first line of defense but are not sufficient across network partitions — CAS makes the store the arbiter of progress monotonicity.

### Positive Consequences

- Duplicate consumers degrade to failed cursor writes instead of silent progress corruption
- Enables at-least-once consumer groups with external partitioning later

### Negative Consequences

- Slightly larger API; consumers must handle CAS failure (re-read and decide)
- Does not by itself prevent duplicate *processing* — only duplicate *progress tracking*

## Links

- `dcb-layer/src/subscribe.rs:29` (`set_cursor`), `dcb-layer/src/lib.rs:154-155`
- `_notes/fw-ex-design.md`
- Related: ADR-0009
