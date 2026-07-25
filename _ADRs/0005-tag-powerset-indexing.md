# Tag-subset (powerset) indexing: one index key per subset of an event's tags

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Queries match events by type and by "has ALL of these tags". With an ordered key-value store, how do we make any tag-combination query fast, and — critically — expressible as key ranges so append conditions can become read-conflict ranges (ADR-0006)?

## Decision Drivers

- Reads and condition probes must be contiguous range scans (no read-time set intersection)
- Conflict detection precision: the ranges a condition reads must match exactly the events the query matches
- FDB transaction budget: 10 MB / 5 s per append

## Considered Options

- Powerset indexing: index key per subset of sorted tags
- Per-tag index + read-time intersection of streams
- External search index (e.g. inverted index service)

## Decision Outcome

Chosen option: "Powerset indexing". On append, `generate_superset_presorted` enumerates all 2^n subsets of the event's sorted+deduped tags (including the empty subset) and writes `pack([ns, "i", tag..., "_", type, versionstamp])` for each. Any query item — tags-only, type-only, or both — then resolves to one contiguous range per (tag-subset × type). Tags are capped at 10 per event (max 1 024 index keys).

### Positive Consequences

- Every read/probe is a single range scan; k-way merge only across OR branches (`read.rs`)
- Conflict ranges are exact — machine-checked equivalence `conflict_iff_matches` (ADR-0013)
- Sorted+deduped tags make subset keys canonical (order-insensitive matching)

### Negative Consequences

- Write amplification 2^n: a 10-tag event writes 1 025 keys; batch sizing must respect the 10 MB txn limit (`lib.rs` "Practical sizing")
- Hard 10-tag cap baked into validation (`append.rs:66`, `Error::TooManyTags`)
- `"_"` becomes a reserved tag value (index separator segment)
- Storage overhead proportional to tag count; challenged in ADR-0016

## Pros and Cons of the Options

### Powerset indexing

- Good, because O(1) ranges per query item; precise conflicts; simple read path
- Bad, because exponential write amplification and tag cap

### Per-tag index + intersection

- Good, because linear write cost, no tag cap
- Bad, because read-time intersection; conflict ranges become over-broad (every tag's range conflicts) — kills precision of ADR-0006

### External search index

- Good, because rich querying
- Bad, because breaks the single-transaction atomicity between write and condition check

## Links

- Implemented by `dcb-layer/src/encoding.rs:32` (`generate_superset_presorted`), `dcb-layer/src/append.rs:34-42`, `dcb-layer/src/query.rs`
- Documented in `dcb-layer/src/lib.rs:134-147`
- Proven by `_doc/model_proof/` (ADR-0013); challenged by ADR-0016
