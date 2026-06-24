# Rappie Fuzzing Experiments

This crate is a small, isolated starting point for Plank differential fuzzing.
It does not fuzz yet. The first goal is to prove the core loop:

1. compile the same Plank source with two backends
2. execute both bytecode outputs in the same EVM
3. compare observable behavior

The current comparison is `sir-debug` versus `sir-release`.

## Layout

- `src/lib.rs`: reusable harness helpers for compiling Plank source, running EVM bytecode, and comparing results.
- `tests/plank_backend_smoke.rs`: fixed and generated-expression backend-diff smoke tests.

The crate is included in the `plankc` workspace so it can call compiler crates directly.

## Expression Model

The first generated-source layer is a tiny expression tree:

```rust
pub enum Expr {
    Const(u64),
    CalldataWord0,
    CalldataWord1,
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}
```

`render_expr` turns this model into Plank syntax. All binary expressions are
parenthesized, and the only generated variables are `a` and `b`, which are defined by
the fixed program template. Arithmetic renders as `+%`, `-%`, and `*%` because Plank
requires explicit wrapping arithmetic for `u256`.

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

This is still deterministic source generation, not fuzzing. The point is to establish
the model-to-Plank rendering boundary before adding `arbitrary` or `cargo-fuzz`.

## Seeded Generation

`generate_expr(seed, max_depth)` builds a bounded expression tree from a tiny
deterministic RNG. The RNG is implemented locally so the crate does not need another
dependency yet.

Generation is intentionally simple:

- depth `0` can only produce leaves: constants, `a`, or `b`
- higher depths can produce leaves or binary operations
- constants are small `u64` values masked to 16 bits

The generated tests run 100 seeds at depth 4 with several fixed calldata pairs. A
failure should include the seed, max depth, calldata label, rendered expression model,
and generated Plank source. That makes a case reproducible as an ordinary Rust test
before it becomes a fuzz corpus entry.

This is not coverage-guided fuzzing. It is a deterministic bridge between the
hand-built expression cases and future `arbitrary`/`cargo-fuzz` support.

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

The fixed smoke test compiles this Plank program:

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

The generated-expression smoke test renders a small set of hand-built `Expr` values
into equivalent program templates and runs each generated program through the same
backend comparison.

The seeded-expression smoke test renders many bounded expressions from fixed seeds
and checks them against `small`, `zero`, and `wrap` calldata cases.

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
- expand the deterministic expression set with more safe `u256` operations
- add `arbitrary` support for the same expression model
- add `cargo-fuzz` only after the compile/run/compare loop is boring
