# Single Rust core crate exposed to Elixir via a Rustler precompiled NIF

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

The store logic must be usable from Elixir (the target application platform) while staying fast and close to the FDB C client.
A previous Go implementation exists (fairway). Where should the core live and how should the BEAM consume it?

## Decision Drivers

- One authoritative implementation of the DCB semantics, reusable across languages
- Quality of the FoundationDB client in each language
- No sidecar processes / network hops for a storage layer
- End-user install ergonomics (no Rust toolchain or libclang required at `mix deps.get`)

## Considered Options

- Rust core crate + Rustler NIF (precompiled binaries)
- Pure Elixir client on the FDB C API
- Keep the Go implementation behind a gRPC/port sidecar

## Decision Outcome

Chosen option: "Rust core crate + Rustler NIF", because `foundationdb-rs` is a mature async client,
the core ships to crates.io as `dcb-layer` (usable by any Rust app), and `RustlerPrecompiled` lets Elixir users
download prebuilt binaries per target instead of compiling.

### Positive Consequences

- Single tested implementation; the Elixir layer (`Dcb.Store`) is a thin wrapper over 7 NIFs
- Precompiled targets (x86_64/aarch64 linux-gnu, aarch64-apple-darwin) — no toolchain for consumers
- Local dev uses a workspace `[patch.crates-io]` so NIF and crate evolve together

### Negative Consequences

- NIF panics can take down the BEAM; FFI boundary must stay careful
- Release pipeline is significantly more complex (ADR-0014)
- Version lockstep required between crate, NIF dependency, and Hex package

## Pros and Cons of the Options

### Rust core + Rustler NIF

- Good, because in-process performance and one core codebase
- Bad, because cross-compilation of `foundationdb-sys` (bindgen/libclang) is painful

### Pure Elixir client

- Good, because no FFI risk
- Bad, because duplicates all DCB logic and no maintained Elixir FDB client of comparable quality

### Go sidecar

- Good, because reuses existing code
- Bad, because adds a process + network hop and serialization layer to every append/read

