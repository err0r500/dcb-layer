# Relax read-path consistency with CausalReadRisky

Status: proposed
Date: 2026-07-02

## Context and Problem Statement

FDB's default read version is obtained via a GRV that round-trips the proxies to
confirm it reflects all prior commits (causal/external consistency). That
round-trip adds latency to every read. Can we drop it on the query read path
(`read()` / `read_all()`) without weakening the append/DCB consistency guarantee?

## Decision Drivers

- Reduce read latency by removing the GRV proxy round-trip.
- Preserve the append/DCB boundary guarantee — appends must stay strongly consistent.
- Prefer a simple, reversible change scoped to the read path.

## Considered Options

- `CausalReadRisky` on the read path only (`read()` / `read_all()`)
- `CausalReadRisky` everywhere, including the append transaction
- Leave default causal reads unchanged
- `ReadYourWritesDisable` on the read path (guarantee-neutral alternative)

## Decision Outcome

Chosen option: "`CausalReadRisky` on the read path only", because `append()` runs
a separate, strongly-consistent transaction and enforces the DCB boundary via
read-conflict ranges (non-snapshot condition reads checked by the resolver at
commit). That guarantee is independent of read-version freshness, so a stale
standalone read is caught at the append boundary and retried. The append path is
left untouched.

### Positive Consequences

- One fewer proxy round-trip per `read()` / `read_all()`.
- Append/DCB guarantee fully preserved; append code unchanged.
- A staler read version only widens the append conflict window (at most a
  spurious retry) — it never misses a real conflict.

### Negative Consequences

- Cross-transaction read-your-writes is not guaranteed on `read()` / `read_all()`:
  a caller may not observe its own just-committed append in an immediately
  following standalone read.

## Pros and Cons of the Options

### `CausalReadRisky` on the read path only

Set the option inside `read_events` / `scan_all_events`, before any read.

- Good, because it removes the GRV round-trip where staleness is safe.
- Good, because append remains strongly consistent (separate transaction).
- Bad, because standalone reads lose cross-transaction read-your-writes.

### `CausalReadRisky` everywhere, including append

- Good, because appends also skip the GRV round-trip.
- Bad, because it weakens the DCB boundary that appends must guarantee — rejected.

### Leave default causal reads unchanged

- Good, because strongest consistency everywhere.
- Bad, because no latency improvement — rejected.

### `ReadYourWritesDisable` on the read path

- Good, because it is guarantee-neutral (read txns never write).
- Bad, because the payoff is a small CPU/alloc saving, not the round-trip — kept
  as a possible future addition, not a substitute.

## Links

- Implements read path — `dcb-layer/src/read.rs` (`read_events`, `scan_all_events`)
- Append stays consistent — `dcb-layer/src/append.rs:129` (separate transaction),
  `dcb-layer/src/append.rs:275` (non-snapshot condition reads → conflict ranges)
