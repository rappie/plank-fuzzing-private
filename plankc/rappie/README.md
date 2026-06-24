# Rappie Fuzzing Experiments

This crate is an isolated Plank differential fuzzing harness. The current target
compares bytecode emitted by the default backend set:

1. decode libFuzzer bytes into a typed Plank program
2. render that program to `.plk` source
3. compile the same source with each backend
4. execute each bytecode output in the same EVM
5. compare observable status and return bytes against the reference backend

## Layout

- `src/lib.rs`: public API for fuzz cases and backend comparison.
- `src/case.rs`: `FuzzCase`, the stable input boundary used by fuzz targets.
- `src/generator/`: typed program model, `arbitrary` decoding, calldata encoding, and rendering.
- `src/compiler.rs`, `src/evm.rs`, and `src/oracle.rs`: compile, execute, and compare pipeline.
- `fuzz/fuzz_targets/plank_backend_program_diff.rs`: libFuzzer target for generated Plank programs.

The crate is included in the `plankc` workspace so it can call compiler crates directly.

## Program Generator

The generator builds a small semantic Plank subset instead of raw syntax nodes. It
currently emits helper functions plus `init { ... }` programs that read 1 to 4 calldata
words, create typed locals, and return one `u256` word.

The internal model is typed and scope-aware:

```rust
Program {
    input_words: u8,
    functions: Vec<Function>,
    stmts: Vec<Stmt>,
    result: U256Value,
}
```

Statements cover typed `let` bindings, mutable assignments, and `if` statements.
Expressions may reference only visible calldata inputs, parameters, helper functions, or
locals of the correct type. Nested expression blocks and `if` branches have their own
local scopes, so block-local refs cannot leak into parent scopes.

The operation set uses deterministic no-stdlib EVM builtins:

- `@evm_not`
- `@evm_add`, `@evm_sub`, `@evm_mul`, `@evm_div`, `@evm_mod`
- `@evm_addmod`, `@evm_mulmod`
- `@evm_and`, `@evm_or`, `@evm_xor`, `@evm_shl`, `@evm_shr`, `@evm_sar`
- `@evm_byte`
- `@evm_eq`, `@evm_lt`, `@evm_gt`, `@evm_slt`, `@evm_sgt`, `@evm_iszero`

The generator also emits full-width 256-bit constants, helper function calls, branch-local
work in `if` expressions, and mutable local updates. Storage, logs, external EVM calls,
loops, `run`, structs, tuples, imports, and comptime features are intentionally deferred
until the oracle can check the extra behavior they expose.

## Rendered Source Shape

Generated source is deterministic and readable. A typical program looks like:

```plk
const f0 = fn(x0: u256, x1: bool) u256 {
    if x1 {
        let v0 = @evm_addmod(x0, x0, x0);
        v0
    } else {
        x0
    }
};

init {
    let in0 = @evm_calldataload(0);
    let in1 = @evm_calldataload(32);

    let mut v0 = @evm_add(in0, in1);
    let b0 = @evm_lt(v0, in0);
    v0 = {
        let v1 = 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff;
        v1
    };
    let v2 = f0(v0, b0);

    let out = @malloc_uninit(32);
    @mstore32(out, v2);
    @evm_return(out, 32);
}
```

Calldata is encoded as one 32-byte big-endian EVM word per generated input.

## Compile And Oracle Path

`compile_plank_source(source, backend, optimizations)` compiles a virtual `main.plk`
file in memory with `plank_source::source_fs::InMemoryFs`; no temporary source file is
written.

The high-level pipeline is:

```text
Plank source -> HIR -> MIR -> bytecode -> revm execution
```

The default oracle compares:

```text
sir-debug
sir-release
sona-o0
```

`sir-debug` is the reference backend. `sir-release` shares the SIR lowering path, while
`sona-o0` exercises the Sonatina lowering and codegen path. Additional Sona specs
(`sona-o1`, `sona-os`, and `sona-o2`) are available for future optimization-level
targets.

`run_bytecode(bytecode, calldata)` installs bytecode at a fixed in-memory account and
executes a call transaction with the generated calldata.

The normalized execution result is:

```rust
pub struct EvmRunResult {
    pub success: bool,
    pub output: Vec<u8>,
}
```

Gas, storage, logs, and account state are ignored for now.

## Cargo Fuzz

Install the runner once:

```bash
cargo install cargo-fuzz
```

Build the target from `plankc/rappie/`:

```bash
cargo +nightly fuzz build plank_backend_program_diff
```

Run it:

```bash
cargo +nightly fuzz run plank_backend_program_diff
```

Replay a saved crash:

```bash
cargo +nightly fuzz run plank_backend_program_diff fuzz/artifacts/plank_backend_program_diff/<crash-file>
```

On a backend mismatch, the panic output includes the decoded `FuzzCase`, generated
Plank source, and backend diff error.

## Running Tests

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
