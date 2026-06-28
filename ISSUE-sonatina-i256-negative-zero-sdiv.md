# Issue: Sonatina Creates Negative Zero While Folding EVM Signed Division

## Summary

The `plank_sol_program_diff` fuzz target found a real compiler crash in the
Sona/Sonatina backend path while compiling a generated Plank program.

This was not a Solidity mismatch and not a Plank semantic bug. The same program
compiled successfully through the SIR backends, but the Sona backend panicked
inside Sonatina while SCCP folded an `evm_sdiv` instruction.

The bad pattern is:

```text
evm_sdiv negative_value positive_value
```

where the absolute quotient is zero. For example, EVM signed division of `-1 / 5`
should produce canonical zero.

Sonatina instead created an internal `I256` value with this shape:

```text
I256 { is_negative: true, abs: 0 }
```

That "negative zero" later reached `I256::to_u256()`. Since `to_u256()` lowers
negative values with two's-complement arithmetic:

```rust
!self.abs + U256::one()
```

negative zero becomes:

```text
!0 + 1
```

which overflows `primitive_types::U256` in debug/fuzzer builds.

## Affected Path

- Fuzz target:

```text
/workspace/plankc/rappie-sol/fuzz/fuzz_targets/plank_sol_program_diff.rs
```

- Plank backend under test:

```text
sona-o0
sona-o1
sona-os
sona-o2
```

- Crashing upstream component:

```text
Sonatina
```

- Current upstream checkout tested:

```text
/tmp/sonatina-latest
commit 039a9f530856a7c0097ffc9dad904eed3bf33fe3
```

- Plank dependency checkout observed in the panic:

```text
/home/vscode/.cargo/git/checkouts/sonatina-a154b53739ea70c3/9c4e1a7
```

- Faulty source file:

```text
crates/ir/src/bigint.rs
```

- Crashing path:

```text
sonatina_ir::bigint::I256::to_u256
<sonatina_ir::bigint::I256 as core::hash::Hash>::hash
sonatina_ir::dfg::DataFlowGraph::make_imm_value
sonatina_codegen::optim::sccp::SccpSolver::fold
```

## Original Fuzz Artifact

The crashing input was:

```text
/workspace/plankc/rappie-sol/fuzz/artifacts/plank_sol_program_diff/crash-1eb29237a43e2c8d2598def88a32dc7f2b2fbfc4
```

Replay command:

```sh
cd /workspace/plankc/rappie-sol
cargo +nightly fuzz run plank_sol_program_diff \
  fuzz/artifacts/plank_sol_program_diff/crash-1eb29237a43e2c8d2598def88a32dc7f2b2fbfc4 -- -runs=1
```

The crash was a compiler panic, not an execution mismatch.

## Minimized Plank Reproducer

```plk
init {
    let ptr = @malloc_uninit(32);
    let mut value = @evm_calldataload(4);
    value = @evm_or(value, 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff);
    value = @evm_sdiv(
        value,
        0x5555555555555555555555555555555555555555555555555555555555555555,
    );
    @mstore32(ptr, value);
    @evm_return(ptr, 32);
}
```

Reproduction command before the fix:

```sh
cd /workspace/plankc
RUST_BACKTRACE=1 cargo run -q -p plank -- \
  build /tmp/sona-sdiv-or-neg-zero.plk --backend sona -OO0
```

Observed failure:

```text
thread 'main' panicked at primitive-types-0.14.0/src/lib.rs:43:1:
arithmetic operation overflow
```

Important stack frames:

```text
sonatina_ir::bigint::I256::to_u256
<sonatina_ir::bigint::I256 as core::hash::Hash>::hash
sonatina_ir::dfg::DataFlowGraph::make_imm_value
sonatina_codegen::optim::sccp::SccpSolver::fold
```

## Minimized Sonatina IR Reproducer

The issue also reproduces without Plank by feeding Sonatina this IR:

```sntn
target = "evm-ethereum-osaka"

func public %neg_zero(v0.i256) -> i256 {
    block0:
        v1.i256 = or v0 -1.i256;
        v2.i256 = evm_sdiv v1 5.i256;
        return v2;
}
```

The `or v0 -1.i256` forces the dividend to all ones, i.e. EVM signed `-1`.
Signed division by `5` should therefore fold to `0`.

Verification accepts the IR:

```sh
/tmp/sonatina-latest/target/debug/sonatina verify \
  /tmp/sonatina-evm-sdiv-neg-zero.sntn
```

Optimization panicked before the fix:

```sh
/tmp/sonatina-latest/target/debug/sonatina optimize \
  /tmp/sonatina-evm-sdiv-neg-zero.sntn
```

After the fix, the optimizer produces the expected canonical zero:

```sntn
target = "evm-ethereum-osaka"

func public %neg_zero(v0.i256) -> i256 {
    block0:
        return 0.i256;
}
```

## Root Cause

`I256` stores signed integers as a sign bit plus an absolute magnitude:

```rust
pub struct I256 {
    is_negative: bool,
    abs: U256,
}
```

`I256::overflowing_div` computes the absolute quotient first:

```rust
let div_abs = self.abs / rhs.abs;
```

Then it applies the result sign:

```rust
match (self.is_negative, rhs.is_negative) {
    (true, true) | (false, false) => (I256::make_positive(div_abs), false),
    _ => (I256::make_negative(div_abs), false),
}
```

For a case like `-1 / 5`, `div_abs` is zero and the operands have opposite
signs, so this constructs:

```rust
I256::make_negative(U256::zero())
```

Before the fix, `make_negative` did not preserve the implicit invariant that
zero must be non-negative:

```rust
pub fn make_negative(abs: U256) -> Self {
    Self {
        is_negative: true,
        abs,
    }
}
```

That lets negative zero escape into normal immediate handling. SCCP folds the
`evm_sdiv`, interns the folded immediate in `DataFlowGraph::make_imm_value`,
and hashing the immediate calls `I256::to_u256()`.

For negative zero, `to_u256()` does this:

```rust
!U256::zero() + U256::one()
```

That addition overflows and causes the panic.

## Source Code Fix

Canonicalize zero at the signed constructor boundary:

```rust
pub fn make_negative(abs: U256) -> Self {
    if abs.is_zero() {
        return Self::zero();
    }

    Self {
        is_negative: true,
        abs,
    }
}
```

Local patch:

```diff
diff --git a/crates/ir/src/bigint.rs b/crates/ir/src/bigint.rs
index aa20fa62..4d97e765 100644
--- a/crates/ir/src/bigint.rs
+++ b/crates/ir/src/bigint.rs
@@ -154,6 +154,10 @@ impl I256 {
     }
 
     pub fn make_negative(abs: U256) -> Self {
+        if abs.is_zero() {
+            return Self::zero();
+        }
+
         Self {
             is_negative: true,
             abs,
```

This is the minimal fix because it repairs the representation invariant at the
constructor used by both division and remainder. Callers can continue asking for
a negative result based on operand signs, while `I256` itself guarantees that
zero has only one representation.

## Recommended Regression Test

A focused unit test in `crates/ir/src/bigint.rs` should cover both direct
construction and the signed division path:

```rust
#[test]
fn make_negative_canonicalizes_zero() {
    let zero = I256::make_negative(U256::zero());

    assert_eq!(zero, I256::zero());
    assert!(!zero.is_negative());
    assert_eq!(zero.to_u256(), U256::zero());
}

#[test]
fn signed_division_with_zero_quotient_returns_canonical_zero() {
    let lhs = I256::make_negative(U256::one());
    let rhs = I256::make_positive(U256::from(5));

    let (result, overflowed) = lhs.overflowing_div(rhs);

    assert!(!overflowed);
    assert_eq!(result, I256::zero());
    assert!(!result.is_negative());
    assert_eq!(result.to_u256(), U256::zero());
}
```

## Verification

With the local Sonatina patch applied in `/tmp/sonatina-latest`, the minimized
IR optimizes successfully:

```sh
/tmp/sonatina-latest/target/debug/sonatina optimize \
  /tmp/sonatina-evm-sdiv-neg-zero.sntn
```

Observed output:

```text
wrote /tmp/sonatina-evm-sdiv-neg-zero.opt.sntn
```

The optimized IR returns canonical zero:

```sntn
target = "evm-ethereum-osaka"

func public %neg_zero(v0.i256) -> i256 {
    block0:
        return 0.i256;
}
```

Sonatina IR tests also pass with the patch:

```sh
cd /tmp/sonatina-latest
cargo test -q -p sonatina-ir
```

Observed result:

```text
98 passed; 0 failed
5 passed; 0 failed
1 passed; 0 failed; 3 ignored
```

The original fuzz artifact also replayed cleanly when Plank was temporarily
patched to use the fixed Sonatina checkout:

```text
compare: ok in 15.729s, plank_backends=37, solidity_calls=1
```

## Upstream Status

GitHub issue search did not find an existing Sonatina issue for this exact
failure mode. Searches for `to_u256`, `evm_sdiv`, `sdiv`, `make_negative`,
`negative zero`, `overflowing_div`, `bigint`, and the panic text did not match
an existing report.

The closest open issue is SCCP-related, but different:

```text
https://github.com/fe-lang/sonatina/issues/231
```

That issue is about SCCP resolving a dynamic array index into a verifier-invalid
immediate. This bug is an `I256` representation invariant violation exposed by
SCCP folding EVM signed division.
