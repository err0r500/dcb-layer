# Subscriptions via a sentinel-key FDB watch plus durable named cursors

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Consumers need to tail the store (react to new appends) and resume after restarts. FDB has no pub/sub; how are live notification and durable progress tracking provided?

## Decision Drivers

- No polling loops in the common case
- No missed wake-ups (race between catching up and arming a notification)
- Restart-safe progress per named consumer
- Stay inside FDB (no extra broker)

## Considered Options

- Sentinel key + FDB watch, cursors as plain keys
- Polling with backoff
- External message broker alongside the store

## Decision Outcome

Chosen option: "Sentinel watch + named cursors". Every append does `SetVersionstampedValue` on a single `lastvs` sentinel key — the value is always 12 fresh bytes, so a registered FDB watch fires on every append. Consumers follow arm-before-catch-up: `register_sentinel_watch` first, then read from their cursor, then `wait_for_sentinel_change` — closing the wake-loss window. Cursors are plain keys under `subs/<name>` with explicit last-writer-wins semantics.

### Positive Consequences

- Push-style tailing with zero polling and zero extra infrastructure
- Cursors are namespaced and isolated by name (`subscribe_tests.rs`)
- Sentinel is one key regardless of throughput — watch cost is O(1)

### Negative Consequences

- Coarse signal: the watch says "something was appended", not what — consumers re-read from their cursor (fine, since reads are cheap range scans)
- Last-writer-wins cursors assume one consumer per name; documented in `lib.rs:154`, challenged in ADR-0019
- A single sentinel key is a (tiny) per-append write hotspot in FDB's key space

## Pros and Cons of the Options

### Sentinel watch + cursors

- Good, because exploits FDB watches; no broker; race-free by protocol
- Bad, because coarse-grained and per-name single-consumer

### Polling

- Good, because trivially simple
- Bad, because latency/throughput tradeoff on every consumer

### External broker

- Good, because rich delivery semantics
- Bad, because dual-write consistency problem between store and broker

## Links

- Implemented by `dcb-layer/src/subscribe.rs`, `dcb-layer/src/append.rs:89-94,156`
- Verified by `dcb-layer/tests/subscribe_tests.rs`
- Challenged by ADR-0019
