# Internal compiler crash when SCCP folds `evm_sdiv` to negative zero

Sonatina panics during SCCP when `evm_sdiv` produces a zero quotient from
operands with different signs. For example, `-1 / 5` should fold to canonical
zero, but it is represented internally as a negative zero.

## Reproducer

Using [Plank](https://github.com/plankevm/plank-monorepo) at commit
`1ddf8ab2e1edfdaa98c32306436a3ce7457a0809`, save this as `repro.plk`:

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
cargo run -q -p plank -- \
  build /path/to/repro.plk --backend sona -OO0
```

The build panics with:

```text
thread 'main' panicked at primitive-types-0.14.0/src/lib.rs:43:1:
arithmetic operation overflow
```

## Suspected Cause

`I256::overflowing_div` computes an absolute quotient of zero, then calls
`I256::make_negative(0)` because the operands have different signs. Hashing the
result calls `I256::to_u256()`, which evaluates `!0 + 1` and overflows.
