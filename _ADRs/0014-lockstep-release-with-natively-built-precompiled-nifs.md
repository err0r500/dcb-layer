# Lockstep tag-driven releases with natively built (no cross) precompiled NIF binaries

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

One version tag must produce a coherent release across three artifacts: the `dcb-layer` crate on crates.io, precompiled NIF binaries on GitHub releases, and the `dcb_layer` Hex package that downloads them. The NIF depends on `foundationdb-sys`, whose bindgen needs a modern libclang — which the standard `cross` images don't have. How is the pipeline structured?

## Decision Drivers

- Version skew between crate / NIF dependency / mix version must be impossible
- `cross` container images ship a libclang too old for `foundationdb-sys` bindgen
- Hex consumers must get checksummed binaries per RustlerPrecompiled's contract
- Local dev must keep using the workspace path dependency

## Considered Options

- Native per-target runners, lockstep version guard, sequential publish jobs
- `cross`-based build matrix
- Ship source-only Hex package (consumers compile the NIF)

## Decision Outcome

Chosen option: "Native runners + lockstep guard" (`.github/workflows/publish.yml`, tag `v*`):
1. `version-guard` — asserts tag == crate version == NIF's `dcb-layer` dep == mix `@version`.
2. `crate` — publishes to crates.io (idempotent skip if already published), then polls the sparse index until live.
3. `build-nif` — per-target **native** runners: ubuntu-22.04 (x86_64-linux-gnu), ubuntu-24.04-arm (aarch64-linux-gnu), macos-14 (aarch64-apple-darwin); `use-cross: ""` skips cross entirely. Deletes the workspace `Cargo.toml`/`Cargo.lock` so the NIF resolves `dcb-layer` from crates.io (the root `[patch.crates-io]` path override is dev-only and never ships). Tarballs attach to the GitHub release.
4. `publish-ex` — runs in dev env (ex_doc available), bootstraps `checksum-Elixir.Dcb.Native.exs` via `mix rustler_precompiled.download --all`, then `mix hex.publish`.

### Positive Consequences

- Impossible to release mismatched versions; publish is re-runnable per job
- Native builds sidestep the libclang problem and produce binaries on real target hardware
- `mix deps.get` consumers never need Rust, clang, or FDB headers

### Negative Consequences

- Target set limited to available native runners — x86_64-darwin was dropped; adding targets means finding runners
- Four sequential jobs; a mid-pipeline failure needs manual re-run (mitigated by idempotent steps)
- `mix hex.build` can't run in regular CI (checksum file exists only at release time) — deliberately skipped in `ci.yml`

## Pros and Cons of the Options

### Native runners + lockstep guard

- Good, because it works with bindgen and real hardware; guard removes a whole failure class
- Bad, because runner availability constrains the target matrix

### cross-based matrix

- Good, because wide target coverage on one runner
- Bad, because libclang in cross images is too old for `foundationdb-sys` — it simply fails

### Source-only Hex package

- Good, because trivial pipeline
- Bad, because every consumer needs Rust + libclang + FDB headers; unacceptable DX

## Links

- Implemented by `.github/workflows/publish.yml`, `.github/workflows/ci.yml`, `dcb-layer-ex/lib/dcb/native.ex`, root `Cargo.toml` (`[patch.crates-io]`)
- Related: ADR-0002
