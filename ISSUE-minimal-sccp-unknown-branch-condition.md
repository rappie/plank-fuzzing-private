# SCCP panics on unknown branch condition during worklist propagation

## Summary

Compiling a small Plank program with the optimized SIR backend can panic in SCCP when a reachable branch condition is still `LatticeValue::Unknown`.

The condition comes from calldata, so it is valid for SCCP to process reachable control flow before all related value propagation has stabilized. In that state the pass should conservatively keep both branch edges reachable, not assert.

## Reproducer

```sh
cat >/tmp/sccp-unknown-branch.plk <<'PLK'
init {
    let in0 = @evm_calldataload(0);
    let b0 = @evm_iszero(in0);
    if b0 {
        let mut b1 = false;
        if b0 {
        }
        if b1 {
        }
    }
    @evm_stop();
}
PLK

cd plankc
cargo run -q -p plank -- build /tmp/sccp-unknown-branch.plk --backend sir-release -O csud
```

## Actual Behavior

The compiler panics in SCCP:

```text
sir/crates/passes/src/optimizations/constant_propagation.rs:190
assertion failed: either != LatticeValue::Unknown
```

## Expected Behavior

The program should compile. While SCCP has not proven which branch edge is taken, both successor edges should remain feasible.

## Suspected Cause

`SCCP::is_edge_reachable` treats an `Unknown` branch condition as an impossible state in debug builds. `Unknown` is a valid transient value during worklist propagation, so the fallback branch should include `Unknown` and allow either successor edge.
