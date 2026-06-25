# Rappie Solidity Differential Fuzzing

This crate is a Plank/Solidity differential fuzzing harness. It generates one
typed semantic program, renders equivalent Plank and Solidity sources, compiles
both, executes both deployed bytecodes in `revm`, and compares success plus return
data.

The Solidity side deliberately renders inline assembly inside a fallback contract
so the generated operations use EVM semantics instead of high-level Solidity
checked arithmetic or ABI dispatch.

## Requirements

Install `cargo-fuzz` once:

```bash
cargo install cargo-fuzz
```

Provide a native `solc` binary. The harness resolves it in this order:

1. `RAPPIE_SOL_SOLC`
2. `SOLC_PATH`
3. `solc` on `PATH`

The wrapper invokes `solc --standard-json` and sends all source over stdin. It
does not write source files, artifacts, project caches, or shared compiler output,
so it is safe to run with multiple libFuzzer workers such as `-fork=12`.

## Running

From `plankc/rappie-sol/`:

```bash
RAPPIE_SOL_SOLC=/path/to/solc cargo +nightly fuzz build plank_sol_program_diff
RAPPIE_SOL_SOLC=/path/to/solc cargo +nightly fuzz run plank_sol_program_diff -- -fork=12
```

Replay a saved crash:

```bash
RAPPIE_SOL_SOLC=/path/to/solc cargo +nightly fuzz run plank_sol_program_diff \
    fuzz/artifacts/plank_sol_program_diff/<crash-file>
```

## Oracle Scope

The first campaign compares only:

- call success
- raw return data

Storage, logs, balances, gas, external calls, ABI dispatch, dynamic memory, and
environment opcodes are intentionally deferred until the oracle records those
observables.
