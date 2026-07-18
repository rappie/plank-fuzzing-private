# `I256` creates negative zero while folding `evm_sdiv`

Sonatina panics during SCCP when `evm_sdiv` produces a zero quotient from
operands with different signs. For example, `-1 / 5` should fold to canonical
zero, but it is represented internally as a negative zero.

## Reproducer

Using [Plank](https://github.com/plankevm/plank-monorepo) at commit
`8c39c8a7f256ffe0793c32486cbe9397d8d8110d`, save this as `repro.plk`:

```plk
init {
    let ptr = @malloc_uninit(32);
    let minus_one = @evm_or(
        @evm_calldataload(4),
        0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff,
    );
    @mstore32(ptr, @evm_sdiv(minus_one, 5));
    @evm_return(ptr, 32);
}
```

Then run:

```sh
cd plankc
RUST_BACKTRACE=1 cargo run -q -p plank -- \
  build /path/to/repro.plk --backend sona -OO0
```

Plank uses Sonatina revision
`9c4e1a7124d30447b7f080be1e0dcf86000bf03a` here. The build panics with:

```text
thread 'main' panicked at primitive-types-0.14.0/src/lib.rs:43:1:
arithmetic operation overflow
```

The relevant stack is:

```text
sonatina_ir::bigint::I256::to_u256
<sonatina_ir::bigint::I256 as core::hash::Hash>::hash
sonatina_ir::dfg::DataFlowGraph::make_imm_value
sonatina_codegen::optim::sccp::SccpSolver::fold
```

## Cause

`I256::overflowing_div` computes an absolute quotient of zero, then calls
`I256::make_negative(0)` because the operands have different signs. Hashing the
result calls `I256::to_u256()`, which evaluates `!0 + 1` and overflows.

I expected the division to fold to canonical zero and compile successfully.
Canonicalizing zero in `I256::make_negative` appears to fix the issue. The same
uncanonicalized constructor is still present on Sonatina `main` at `55ca888`.
