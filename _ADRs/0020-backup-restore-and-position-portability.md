# Backup/restore across clusters and position portability

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

Positions are FDB versionstamps (ADR-0004): the 10-byte transaction version is assigned by *one* cluster's commit pipeline. Versionstamps are embedded in event keys, index keys, the sentinel value, and cursor values. How can an event log be dumped and restored onto another FoundationDB cluster without breaking global order, `after` semantics, and cursors?

## Decision Drivers

- A fresh cluster starts near version 0 — new appends after a naive copy would sort *before* restored events
- Cursors and any externally stored positions must stay meaningful (or be remapped)
- No dump/restore tooling exists in the crate today

## Considered Options

- Document the two operational routes (physical restore + `advanceversion`, logical re-append), no code change
- Add an epoch/incarnation prefix to positions (Record Layer approach)
- Ship a migration tool implementing logical export/re-append with cursor remapping

## Decision Outcome

Proposed: "Document both routes now; adopt the epoch prefix only if cross-cluster moves become routine."

**Route A — physical copy (positions preserved).** `fdbbackup`/`fdbrestore` or DR copies keys verbatim. Hard requirement: before any new append, advance the target cluster's version past the max embedded versionstamp — `fdbcli> advanceversion <v>` with `v` greater than the big-endian value of the newest position's first 10 bytes. `fdbdr` switchover does this automatically; plain restore into a fresh cluster does **not**.

**Route B — logical re-append (positions change).** Paginated `read_all`, re-`append` in order into the target. Fresh versionstamps are assigned, so: build an old→new position mapping and remap cursors; group source events by shared 10-byte tx-version prefix and re-append each group as one batch (user-version bytes preserve intra-batch order); run single-writer or idempotency-guarded — a re-run double-appends. Positions stored outside the store (projections, payloads) break unless remapped by the application.

**Epoch prefix (future design change).** Position becomes `(epoch, versionstamp)` (~13 bytes); a migration bumps the epoch, so old and new data order correctly on any cluster with no `advanceversion` step. This is a key-layout change — it touches ADR-0005/ADR-0008 and hits the missing-layout-version-marker gap flagged in ADR-0008.

### Positive Consequences

- Both operational routes are correct today with zero code changes
- Route A keeps every position stable — cursors and external references survive untouched

### Negative Consequences

- Route A's `advanceversion` step is manual and unguarded — forgetting it silently corrupts ordering
- Route B invalidates all positions; remapping burden falls on the operator/application
- Without the epoch prefix, position portability remains an operational procedure, not a store guarantee

## Pros and Cons of the Options

### Document routes only

- Good, because zero code, both routes are sound
- Bad, because footgun-prone (manual `advanceversion`, hand-rolled remapping)

### Epoch/incarnation prefix

- Good, because portability becomes a store guarantee; no version dance
- Bad, because layout migration for all existing data and wider position type across the API and NIF

### Migration tool

- Good, because packages Route B safely (ordering, batching, cursor remap, idempotency)
- Bad, because meaningful tooling surface to build and maintain before the need is proven

## Links

- `dcb-layer/src/append.rs:159-174` (position construction), `dcb-layer/src/encoding.rs` (versionstamps in keys/values), `dcb-layer/src/subscribe.rs` (cursor values)
- Related: ADR-0004, ADR-0005, ADR-0008
- FDB Record Layer incarnation approach: https://foundationdb.github.io/fdb-record-layer/
