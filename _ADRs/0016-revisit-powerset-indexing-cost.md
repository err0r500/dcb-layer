# Revisit powerset indexing: write amplification, the 10-tag cap, and type discovery

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

Powerset indexing (ADR-0005) costs 2^n index keys per event and hard-caps tags at 10. Additionally, tags-only queries must first *discover* types by scanning the `"_"` sentinel segment (`discover_types_in_tag_subspace`) — cost grows with the number of distinct types under a tag subset. Is the trade still right as data models grow richer?

## Decision Drivers

- 10-tag events write 1 025 keys — pressure on the 10 MB txn budget caps batch sizes
- The 10-tag limit is a modeling constraint leaking into applications
- Type discovery adds a pre-scan to every tags-only read and probe
- Any change must preserve `conflict_iff_matches` (ADR-0013) — precision AND completeness

## Considered Options

- Keep powerset indexing as is
- Selective materialization: index only subsets up to size k (queries with more tags fall back to scan+filter over the largest indexed subset)
- Per-tag index + read-time intersection
- Add a type registry to eliminate discovery scans

## Decision Outcome

Chosen option: none yet — this ADR frames the evaluation. Any alternative must first be re-proven in the Lean model (the cheap place to fail). Notes:
- Per-tag + intersection breaks conflict precision (probes would conflict on every tag range), so it likely dies at the proof stage.
- Selective materialization keeps precision for queries ≤ k tags and degrades gracefully; most real queries use 1–3 tags.
- A type registry (small subspace mapping type → id) removes discovery scans and could shorten every index key, but adds a coordination point.

### Positive Consequences (if changed)

- Lift or soften the 10-tag cap; smaller transactions; larger safe batches

### Negative Consequences (if changed)

- Migration of existing index data (no layout version marker — see ADR-0008)
- Model, proof, tests, and hot paths all touched

## Links

- `dcb-layer/src/encoding.rs:32`, `dcb-layer/src/query.rs:82`, `dcb-layer/src/lib.rs:142-147`
- Related: ADR-0005, ADR-0008, ADR-0013
