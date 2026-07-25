# Global event position from FDB versionstamps (10-byte tx version + 2-byte batch index)

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Every stored event needs a globally ordered, unique position usable as a cursor and as the `after` bound in append conditions. How is that position generated without serializing writers?

## Decision Drivers

- Strict global order across concurrent writers
- No extra read or counter key in the write path (a counter would be a write hotspot)
- Stable intra-batch ordering for multi-event appends

## Considered Options

- FDB versionstamps via `SetVersionstampedKey` atomic ops
- Dedicated sequence counter key (read-increment-write)
- Client-generated IDs (ULID / timestamp-based)

## Decision Outcome

Chosen option: "FDB versionstamps", because FDB fills the 10-byte transaction version at commit time — order is assigned by the commit pipeline itself, with zero reads and zero contention. The 2-byte user version carries the batch index (big-endian u16), ordering events within one append and capping batches at 65 535 events.

### Positive Consequences

- `Versionstamp = [u8; 12]` sorts naturally as bytes; keys sort in commit order
- `append` returns the last event's position by combining the commit's tx version with `n-1` (`append.rs:169`), captured from a versionstamp future taken before commit
- Positions double as pagination cursors (`ReadOptions.after`) and condition bounds

### Negative Consequences

- Positions are opaque and only meaningful relative to one FDB cluster — no wall-clock meaning, not portable across clusters/restores
- Incomplete-versionstamp plumbing (offset suffixes, `extract_vs_from_key`) adds encoding complexity (ADR-0008)

## Pros and Cons of the Options

### FDB versionstamps

- Good, because contention-free, commit-ordered, unique
- Bad, because cluster-bound opacity

### Sequence counter key

- Good, because human-readable dense sequence
- Bad, because every append conflicts on the counter — a global write bottleneck

### Client-generated IDs

- Good, because no server coordination
- Bad, because clock skew breaks strict ordering — unacceptable for `after` semantics

## Links

- Implemented by `dcb-layer/src/types.rs:9`, `dcb-layer/src/append.rs:30-45,159-174`, `dcb-layer/src/encoding.rs`
- Cross-cluster portability challenged by ADR-0020
