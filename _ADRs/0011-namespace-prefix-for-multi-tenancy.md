# Store isolation via a namespace prefix on every key

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Several applications (and thousands of test runs) must share one FoundationDB cluster without seeing each other's events, cursors, or watches. How is isolation achieved?

## Decision Drivers

- Multiple stores per cluster with zero coordination
- Test isolation (each test gets a throwaway store)
- No dependency on FDB's directory layer complexity for a first version

## Considered Options

- Plain namespace string as first tuple element of every key
- FDB directory layer
- One cluster per application

## Decision Outcome

Chosen option: "Namespace string prefix". `FdbStore::new(db, namespace)` scopes every key — events (`e`), indexes (`i`), sentinel (`lastvs`), cursors (`subs`) — under `pack([namespace, ...])`. Two stores with different namespaces are fully disjoint key ranges: reads, conflict ranges, and watches cannot interact.

### Positive Consequences

- Multi-tenancy for free; conflict isolation comes from key-range disjointness
- Tests create a unique namespace per test (timestamp + counter in `tests/common/mod.rs`) and run concurrently against one container
- Human-readable prefixes when inspecting the cluster

### Negative Consequences

- Long namespace strings inflate every key (and every one of the 2^n index keys)
- No built-in namespace enumeration/deletion — cleanup is a manual range clear
- No protection against two apps choosing the same namespace

## Pros and Cons of the Options

### Namespace prefix

- Good, because trivial, transparent, and sufficient
- Bad, because key-size overhead vs the directory layer's short prefixes

### FDB directory layer

- Good, because short allocated prefixes and managed hierarchy
- Bad, because extra metadata transactions and API complexity for little gain at current scale

### Cluster per app

- Good, because hard isolation
- Bad, because operational cost multiplies per app

## Links

- Implemented by `dcb-layer/src/types.rs:84` (`FdbStore::new`), `dcb-layer/src/encoding.rs`
- Used by `dcb-layer/tests/common/mod.rs` (`make_store`)
