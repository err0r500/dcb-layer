# Testing against a real FoundationDB via testcontainers, sharing one container across test binaries

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

The store's correctness lives in FDB-specific behavior: conflict ranges, versionstamps, watches, retry semantics. Mocks cannot exercise any of that. How are the tests made both real and fast?

## Decision Drivers

- Concurrency and condition semantics must be tested against the real MVCC engine
- `cargo test` runs multiple test binaries, each with independent tokio runtimes
- Container startup (~seconds) must be paid once, not per binary or per test

## Considered Options

- Real FDB in testcontainers, one shared container, per-test namespaces
- Mock/fake store trait for unit tests
- Container per test binary

## Decision Outcome

Chosen option: "Shared real FDB container". `tests/common/mod.rs` starts `foundationdb/foundationdb:7.4.5` (arm tag on aarch64) inside a dedicated OS thread owning a single-thread tokio runtime that parks forever (`std::future::pending`) — so the container outlives every per-test runtime and is never dropped mid-run. A cross-process lock file elects one test binary to start the container and run `fdbcli configure new single ssd`; the others wait for the cluster file. Each test gets a unique namespace (ADR-0011), so all tests run concurrently against one cluster. Docker/Colima sockets are auto-detected.

### Positive Consequences

- Tests assert the actual invariants: one-winner concurrency, condition atomicity, watch firing, ordering
- One container per `cargo test` invocation regardless of binary count
- No mock drift — the test double *is* the production engine

### Negative Consequences

- Tests require Docker; CI must provision the FDB client library
- 321 lines of nontrivial infrastructure (nested-runtime and cross-process coordination) to maintain
- No fast pure-unit path for store logic (related: ADR-0018); inline unit tests exist only for encoding/query helpers

## Pros and Cons of the Options

### Shared real container

- Good, because maximal fidelity at near-unit-test speed after warmup
- Bad, because infrastructure complexity and Docker dependency

### Mocked store

- Good, because instant and hermetic
- Bad, because the load-bearing behavior (MVCC conflicts) is exactly what a mock fakes

### Container per binary

- Good, because no cross-process coordination
- Bad, because multiplies startup cost and resource use

## Links

- Implemented by `dcb-layer/tests/common/mod.rs`
- Exercised by all files in `dcb-layer/tests/`
- Related: ADR-0011, ADR-0015, ADR-0018
