# Adopt property-based testing (or drop the unused proptest dependency)

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

`proptest 1` sits in `dcb-layer`'s dev-dependencies but zero property tests exist. Meanwhile the Lean proof (ADR-0013) verifies the *model*, not the Rust implementation — the encoding/query-translation gap is exactly what randomized testing covers well. Keep the dependency and use it, or remove it?

## Decision Drivers

- Dead dependencies mislead readers about the test strategy
- The model–implementation gap of ADR-0013 is currently covered only by hand-picked integration cases
- Pure functions (`generate_superset_presorted`, key pack/unpack, `sort_tags`, range construction) are ideal property-test targets — no FDB needed

## Considered Options

- Adopt: write property tests mirroring the Lean spec against the pure Rust functions
- Drop: remove `proptest` from dev-dependencies
- Status quo (unused dependency)

## Decision Outcome

Chosen option: "Adopt", because the highest-value properties are cheap and FDB-free:
- encode/decode round-trips for event values and keys (incl. `extract_vs_from_key` vs full `unpack`)
- for random events and query items: `query ranges cover event's index keys ⟺ matchesQuery(event, query)` — the Rust-side twin of `conflict_iff_matches`
- subset generation: canonical, complete (2^n), order-insensitive under tag permutation

If not adopted within a release cycle, drop the dependency instead.

### Positive Consequences

- Narrows the Lean-model-to-Rust gap with executable evidence
- Catches encoding regressions before they become silent index corruption

### Negative Consequences

- Property formulations duplicate part of the Lean spec (two places to update on layout change)

## Links

- `dcb-layer/Cargo.toml` (dev-dependencies), `dcb-layer/src/encoding.rs`, `dcb-layer/src/query.rs`
- Related: ADR-0013, ADR-0005
