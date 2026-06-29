# SCCP misses outputs from a late reachable predecessor into a merge block

## Summary

The optimized SIR backend can miscompile a reachable merge block when SCCP discovers one predecessor edge after the successor block has already been marked reachable.

In the failing case, `sir-debug` returns `1` for nonzero calldata, but `sir-release -O csud` returns `0`.

## Reproducer

```plk
init {
    let in0 = @evm_calldataload(0);
    let b0 = @evm_iszero(in0);
    let mut v0 = 2;
    if b0 {
        v0 = 0;
    } else {
    }
    v0 = @evm_div(v0, v0);

    let out = @malloc_uninit(32);
    @mstore32(out, v0);
    @evm_return(out, 32);
}
```

Run the program with any nonzero calldata word, for example:

```text
0000000000000000000000000000000000000000000000000000000000000001
```

## Actual Behavior

With `sir-release -O csud`, the program returns:

```text
0x0000000000000000000000000000000000000000000000000000000000000000
```

## Expected Behavior

For nonzero calldata, `b0` is false, so the `v0 = 0` assignment must not run. The program should compute `2 / 2` and return:

```text
0x0000000000000000000000000000000000000000000000000000000000000001
```

This is what `sir-debug` returns.

## Suspected Cause

`SCCP::mark_reachable` only flows predecessor block outputs when the successor block is first marked reachable. That is unsound for merge blocks: each feasible predecessor edge must contribute its outputs to the successor inputs, even if the successor block was already reachable.

The edge-output flow should happen for every reachable edge, while the CFG worklist enqueue should still happen only the first time a block becomes reachable.
