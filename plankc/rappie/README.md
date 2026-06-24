# Rappie Fuzzing Experiments

This crate is a small, isolated starting point for Plank differential fuzzing.
It does not fuzz yet. The first goal is to prove the core loop:

1. compile the same Plank source with two backends
2. execute both bytecode outputs in the same EVM
3. compare observable behavior

The current comparison is `sir-debug` versus `sir-release`.

## Layout

- `src/lib.rs`: reusable harness helpers for compiling Plank source, running EVM bytecode, and comparing results.
- `tests/plank_backend_smoke.rs`: one fixed Plank program used as a backend-diff smoke test.

The crate is included in the `plankc` workspace so it can call compiler crates directly.

## Compile Path

`compile_plank_source(source, backend)` compiles a virtual `main.plk` file in memory.
It uses `plank_source::source_fs::InMemoryFs`, so no temporary source file is written.

The function follows the same high-level path as the Plank CLI:

```text
Plank source
  -> parse/load project
  -> lower HIR
  -> evaluate HIR into MIR
  -> emit bytecode with selected backend
```

The selected backend is passed as `plank_driver::BackendKind`. The smoke test uses:

```text
BackendKind::SirDebug
BackendKind::SirRelease
```

The EVM version is fixed to `EvmVersion::Osaka`, matching the current CLI default.

## EVM Runner

`run_bytecode(bytecode, calldata)` executes already-compiled bytecode with `revm`.
It inserts the bytecode at a fixed target address in an in-memory `CacheDB<EmptyDB>`,
then sends a call transaction to that address.

The result is normalized into:

```rust
pub struct EvmRunResult {
    pub success: bool,
    pub output: Vec<u8>,
}
```

This intentionally ignores gas, storage, logs, and account state for now. The first
oracle only checks whether both backends agree on success/revert status and output
bytes.

## Smoke Test

The current test compiles this Plank program:

```plk
init {
    let a = @evm_calldataload(0);
    let b = @evm_calldataload(32);
    let result = a +% b;

    let out = @malloc_uninit(32);
    @mstore32(out, result);
    @evm_return(out, 32);
}
```

It provides calldata containing two 32-byte words, `7` and `3`, then compares the
`sir-debug` and `sir-release` execution results.

The `+%` operator is used because Plank requires explicit wrapping arithmetic for
`u256` values.

## Running

From `plankc/`:

```bash
cargo test -p rappie
```

Formatting check:

```bash
cargo +nightly fmt -p rappie --check
```

The first dependency fetch may need GitHub HTTPS access because the workspace includes
the Sonatina backend dependency.

## Next Steps

Good small follow-ups:

- add one or two more fixed Plank smoke programs
- extract a tiny expression model
- render generated expressions into the fixed Plank template
- run deterministic generated cases before adding `cargo-fuzz`
- add `cargo-fuzz` only after the compile/run/compare loop is boring
