# Rappie Fuzzing Experiments

This crate is a small, isolated Plank differential fuzzing harness. The current
target compares bytecode emitted by the `sir-debug` and `sir-release` backends:

1. compile the same Plank source with two backends
2. execute both bytecode outputs in the same EVM
3. compare observable behavior

## Layout

- `src/lib.rs`: thin public API for fuzz cases and backend comparison.
- `src/case.rs`: `FuzzCase`, `arbitrary` decoding, and calldata construction.
- `src/expr.rs` and `src/program.rs`: generated expression AST and Plank rendering.
- `src/compiler.rs`, `src/evm.rs`, and `src/oracle.rs`: compile, execute, and compare pipeline.
- `fuzz/fuzz_targets/plank_backend_expr_diff.rs`: libFuzzer target for generated expression programs.

The crate is included in the `plankc` workspace so it can call compiler crates directly.

## Expression Model

The generated-source layer is a tiny expression tree:

```rust
enum Expr {
    Const(u64),
    CalldataWord0,
    CalldataWord1,
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}
```

`BinaryOp` covers wrapping arithmetic (`+%`, `-%`, `*%`) and bitwise operators (`^`,
`&`, `|`). Rendering parenthesizes every binary expression, and the only generated
variables are `a` and `b`, which are defined by the fixed program template.

`render_program` inserts the rendered expression into this init-only template:

```plk
init {
    let a = @evm_calldataload(0);
    let b = @evm_calldataload(32);
    let result = <rendered expr>;

    let out = @malloc_uninit(32);
    @mstore32(out, result);
    @evm_return(out, 32);
}
```

This keeps generation structure-aware: every decoded fuzz input renders to syntactically
valid Plank for this small program shape.

## Arbitrary Decoding

`FuzzCase` is the structured input shape decoded by the fuzz target:

```rust
pub struct FuzzCase {
    // private fields
}
```

It implements `arbitrary::Arbitrary` manually and exposes `source()` and `calldata()`
for the fuzz target. The expression decoder uses bounded recursion with a maximum depth
of 4, emits only valid `Expr` nodes, and keeps constants small by decoding `u16` values.

The pipeline is:

```text
bytes -> FuzzCase -> Expr -> Plank source -> backend diff
```

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

The selected backend is passed as `plank_driver::BackendKind`. The default oracle uses:

```text
BackendKind::SirDebug
BackendKind::SirRelease
```

The EVM version is fixed to `EvmVersion::Osaka`, matching the current CLI default.

## Execution Oracle

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
bytes. Public comparison APIs return `HarnessError`, which distinguishes compilation
failures, execution failures, and backend mismatches.

## Cargo Fuzz

The coverage-guided target is `plank_backend_expr_diff`. It feeds libFuzzer bytes
through the `FuzzCase` decoder:

```text
libFuzzer bytes -> FuzzCase -> Plank source -> sir-debug/sir-release backend diff
```

Install the runner once:

```bash
cargo install cargo-fuzz
```

Build the target from `plankc/rappie/`:

```bash
cargo +nightly fuzz build plank_backend_expr_diff
```

Run it:

```bash
cargo +nightly fuzz run plank_backend_expr_diff
```

Replay a saved crash:

```bash
cargo +nightly fuzz run plank_backend_expr_diff fuzz/artifacts/plank_backend_expr_diff/<crash-file>
```

On a backend mismatch, the panic output includes the decoded `FuzzCase`, generated
Plank source, and backend diff error. Generated corpus, artifact, coverage, and fuzz
target build directories are ignored under `fuzz/`.

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

- add a small seed corpus or replay/debug runner for saved fuzz inputs
- expand the expression model with more safe `u256` operations
- compare additional backends once the current oracle is stable
