# Rappie Solidity Differential Fuzzing

This crate is a Plank/Solidity differential fuzzing harness. It decodes fuzzer
bytes into one structured semantic program, renders equivalent Plank and
Solidity/Yul sources, compiles both, executes both deployed bytecodes in one
shared `revm` state, and compares the observable trace.

The Solidity side deliberately renders inline assembly inside a payable fallback
contract so the generated operations use EVM semantics instead of high-level
Solidity checked arithmetic.

## Requirements

Install `cargo-fuzz` once:

```bash
cargo install cargo-fuzz
```

Provide native `solx` and `solc` binaries. `solx` remains the Solidity
reference compiler. `solc` is also run as a strict Solidity peer backend across
optimizer and via-IR modes.

The harness resolves `solx` in this order:

1. `RAPPIE_SOL_SOLX`
2. `SOLX_PATH`
3. `solx` on `PATH`

The harness resolves `solc` in this order:

1. `RAPPIE_SOL_SOLC`
2. `SOLC_PATH`
3. `solc` on `PATH`

The wrapper invokes `solx --standard-json --threads 1` and sends all source over
stdin. It does not write source files, artifacts, project caches, or shared
compiler output, so it is safe to run with multiple libFuzzer workers such as
`-fork=12`. `--threads 1` avoids nested compiler parallelism while libFuzzer is
already running multiple worker processes.

The `solc` peers are invoked with `solc --standard-json`:

- `solc-noopt-legacy`: optimizer disabled, `viaIR = false`
- `solc-noopt-via-ir`: optimizer disabled, `viaIR = true`
- `solc-opt-legacy`: optimizer enabled with `runs = 200`, `viaIR = false`
- `solc-opt-via-ir`: optimizer enabled with `runs = 200`, `viaIR = true`

## Backend Configuration

By default, the oracle runs the `solx-reference` reference backend, every `solc`
peer above, and every configured Plank SIR/Sona backend. To filter that matrix,
copy `backends.example.toml` to `backends.toml` in this directory, or set
`RAPPIE_SOL_BACKENDS_CONFIG=/path/to/backends.toml`.

The `reference` value must name a known Solidity backend and remains required as
the comparison oracle. Entries in `[solidity]` and `[plank]` enable or disable
known backends by name; omitted entries keep their default value. Unknown backend
names or a config that disables every candidate backend are reported as errors.

## Running

From `plankc/rappie-sol/`:

```bash
mkdir -p fuzz/corpus/plank_sol_program_diff
cp fuzz/seeds/plank_sol_program_diff/* fuzz/corpus/plank_sol_program_diff/
RAPPIE_SOL_SOLX=/path/to/solx cargo +nightly fuzz build plank_sol_program_diff
RAPPIE_SOL_SOLX=/path/to/solx RAPPIE_SOL_SOLC=/path/to/solc \
    cargo +nightly fuzz run plank_sol_program_diff -- -fork=12
```

The seed copy is optional once a local corpus already exists, but it helps a new
corpus reach valid structured programs immediately.

Regenerate the tracked seed corpus after generator changes:

```bash
RAPPIE_SOL_SOLX=/path/to/solx RAPPIE_SOL_SOLC=/path/to/solc \
    cargo run --bin seedgen -- --target 160
```

`seedgen` decodes deterministic candidate byte buffers, buckets the resulting
cases by generated-program shape, validates selected candidates through the full
Plank/Solidity oracle, and rewrites `fuzz/seeds/plank_sol_program_diff/`.

Replay a saved crash:

```bash
RAPPIE_SOL_SOLX=/path/to/solx RAPPIE_SOL_SOLC=/path/to/solc \
    cargo +nightly fuzz run plank_sol_program_diff \
        fuzz/artifacts/plank_sol_program_diff/<crash-file>
```

## Oracle Scope

The campaign executes one to four calls against the same deployed bytecode and
compares:

- per-call success
- per-call raw return data
- per-call emitted logs
- final nonzero storage slots

Balances and gas are not currently oracle outputs.

Every generated program is compiled and executed through the `solx` reference,
all strict `solc` peers, and all configured Plank SIR/Sona backends. Any compile
failure, execution failure, or trace mismatch in one backend rejects the case.

## Generated Programs

The generator emits bounded programs that can run under `-fork=12` without shared
compiler artifacts. Each case renders a Plank source set and one Solidity/Yul
oracle from the same structured case. Generated Plank may be a single `main.plk`
or a multi-file in-memory project using the registered `gen` module. Cases that
exercise core-operator lowering load the repo `std/` tree into the same in-memory
filesystem and register it as `std`.

Each case may use raw fallback calldata or selector dispatch with one to six
entries, then executes one to four selected calls. Executed entries combine:

- multi-word returns plus non-32-byte return/revert data
- short, unaligned, and copied calldata
- scratch memory stores/loads with widths from 1 to 32 bytes
- memory copies and `keccak256`
- bounded loops
- persistent storage stores/loads, including repeated slots across calls
- transient storage stores/loads
- `log0` through `log4`
- deterministic environment opcodes such as caller, callvalue, chainid,
  timestamp, block number, basefee, prevrandao, and gaslimit
- calls, staticcalls, and delegatecalls into deterministic helper contracts
  installed in the `revm` database
- returndata size/copy/hash paths
- create/create2 with fixed initcode
- external code size/hash/copy for deterministic helper and empty accounts
- signed arithmetic, signed comparisons, and signed shifts
- edge constants such as zero, one, byte masks, signed min/max, and `u256::MAX`
- imported helper files and grouped imports
- generated structs, tuples, compound literals, field reads, and field updates
- comptime type reflection builtins such as `@field_count`, `@field_name`,
  `@field_type`, `@type_index`, `@uninit`, `@in_comptime`, and
  `@active_evm_version`
- cbytes/string builtins such as `@slice_cbytes`, `@padded_read_cbytes`,
  `@concat_cbytes`, `@keccak256_cbytes`, and `@sha256_cbytes`
- high-level wrapping, comparison, shift, equality, bitwise, and unary operators
- std-backed checked operators when core ops are registered
- normal and nested helper functions over primitive and compound values
- comments, whitespace, binary literals, and hex literals
