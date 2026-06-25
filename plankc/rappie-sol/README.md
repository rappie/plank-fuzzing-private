# Rappie Solidity Differential Fuzzing

This crate is a Plank/Solidity differential fuzzing harness. It decodes fuzzer
bytes into one structured semantic program, renders equivalent Plank and
Solidity/Yul sources, compiles both, executes both deployed bytecodes in `revm`,
and compares the observable execution result.

The Solidity side deliberately renders inline assembly inside a payable fallback
contract so the generated operations use EVM semantics instead of high-level
Solidity checked arithmetic.

## Requirements

Install `cargo-fuzz` once:

```bash
cargo install cargo-fuzz
```

Provide a native `solx` binary. The harness resolves it in this order:

1. `RAPPIE_SOL_SOLX`
2. `SOLX_PATH`
3. `solx` on `PATH`

The wrapper invokes `solx --standard-json --threads 1` and sends all source over
stdin. It does not write source files, artifacts, project caches, or shared
compiler output, so it is safe to run with multiple libFuzzer workers such as
`-fork=12`. `--threads 1` avoids nested compiler parallelism while libFuzzer is
already running multiple worker processes.

## Running

From `plankc/rappie-sol/`:

```bash
mkdir -p fuzz/corpus/plank_sol_program_diff
cp fuzz/seeds/plank_sol_program_diff/* fuzz/corpus/plank_sol_program_diff/
RAPPIE_SOL_SOLX=/path/to/solx cargo +nightly fuzz build plank_sol_program_diff
RAPPIE_SOL_SOLX=/path/to/solx cargo +nightly fuzz run plank_sol_program_diff -- -fork=12
```

The seed copy is optional once a local corpus already exists, but it helps a new
corpus reach valid structured programs immediately.

Regenerate the tracked seed corpus after generator changes:

```bash
RAPPIE_SOL_SOLX=/path/to/solx cargo run --bin seedgen -- --target 48
```

`seedgen` decodes deterministic candidate byte buffers, buckets the resulting
cases by generated-program shape, validates selected candidates through the full
Plank/Solidity oracle, and rewrites `fuzz/seeds/plank_sol_program_diff/`.

Replay a saved crash:

```bash
RAPPIE_SOL_SOLX=/path/to/solx cargo +nightly fuzz run plank_sol_program_diff \
    fuzz/artifacts/plank_sol_program_diff/<crash-file>
```

## Oracle Scope

The campaign compares:

- call success
- raw return data
- emitted logs
- changed storage slots

Balances and gas are not currently oracle outputs.

## Generated Programs

The generator emits bounded programs that can run under `-fork=12` without shared
compiler artifacts. Each case may use raw fallback calldata or selector dispatch
with one to four entries. Executed entries combine:

- multi-word returns and non-32-byte return/revert data
- scratch memory stores, loads, overwrites, and `keccak256`
- bounded loops
- storage stores and loads
- `log0` through `log4`
- deterministic environment opcodes such as caller, callvalue, chainid,
  timestamp, block number, and basefee
- calls/staticcalls into deterministic helper contracts installed in the `revm`
  database
- signed comparisons and signed shifts
- edge constants such as zero, one, byte masks, signed min/max, and `u256::MAX`
