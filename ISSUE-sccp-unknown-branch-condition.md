# Issue: SCCP Panics on Unknown Branch Condition During Worklist Propagation

## Summary

The `plank_backend_program_diff` fuzz target found a compiler crash in the optimized SIR release backend path. This was not a semantic mismatch between backends. The crash was a debug assertion failure in sparse conditional constant propagation (SCCP) while compiling with `sir-release -Ocsud`.

The assertion assumed that a reachable branch condition could not still be `LatticeValue::Unknown`. That assumption is invalid for this SCCP implementation because reachable blocks can be processed while related value propagation is still pending in the worklists. The correct behavior is conservative: if SCCP has not yet proven which branch edge is taken, both branch edges must be considered reachable.

The fix has been applied locally in:

```text
/workspace/plankc/sir/crates/passes/src/optimizations/constant_propagation.rs
```

## Affected Path

- Fuzz target: `rappie/fuzz/fuzz_targets/plank_backend_program_diff.rs`
- Default compared backends at the time of the crash:
  - `sir-debug`
  - `sir-release -Ocsud`
- Crashing pass: `SCCP`
- Crashing function: `SCCP::is_edge_reachable`
- Panic location:

```text
sir/crates/passes/src/optimizations/constant_propagation.rs:190
assertion failed: either != LatticeValue::Unknown
```

## Original Fuzz Artifact

The crashing input was:

```text
/workspace/plankc/rappie/fuzz/artifacts/plank_backend_program_diff/crash-b7d7fe660f405ddc8995ea7b2e55956cd9696e6c
```

Replay command:

```sh
cd /workspace/plankc/rappie
cargo +nightly fuzz run plank_backend_program_diff \
  fuzz/artifacts/plank_backend_program_diff/crash-b7d7fe660f405ddc8995ea7b2e55956cd9696e6c
```

Decoded calldata:

```text
000000000000000000000000000000000000000000000000d1d1d1ae16000002
```

## Original Generated Plank Program

```plk
init {
    let in0 = @evm_calldataload(0);

    let mut b0 = @evm_sgt(in0, in0);
    if b0 {
        let mut b1 = false;
        if b0 {
        } else {
        }
        if b1 {
        } else {
        }
        let mut v0 = 0;
        let mut v1 = 0;
        let mut v2 = 0;
        let mut v3 = 0;
    } else {
    }
    let mut v4 = 0;
    let mut v5 = 0;
    let mut v6 = 0;
    let mut v7 = 0;
    let mut v8 = 0;
    let mut v9 = 0;
    let mut v10 = 0;
    let mut v11 = 0;
    let mut v12 = 0;
    let mut v13 = 0;
    let mut v14 = 0;
    let mut v15 = 0;
    let mut v16 = 0;
    let mut v17 = 0;
    let mut v18 = 0;
    let mut v19 = 0;
    let mut v20 = 0;
    let mut v21 = 0;
    let mut v22 = 0;
    let mut v23 = 0;
    let mut v24 = 0;
    let mut v25 = 0;
    let mut v26 = 0;
    let mut v27 = 0;
    let mut v28 = 0;
    let mut v29 = 0;
    let mut v30 = 0;
    let mut v31 = 0;
    let mut v32 = 0;

    let out = @malloc_uninit(32);
    @mstore32(out, in0);
    @evm_return(out, 32);
}
```

## Minimized Plank Reproducer

This is the reduced program that still reproduced the crash before the fix:

```plk
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
```

Reproduction command before the fix:

```sh
cd /workspace/plankc
cargo run -q -p plank -- build /tmp/min22.plk --backend sir-release -Ocsud
```

This panicked in SCCP. The same program compiled successfully through unoptimized `sir-debug`, unoptimized `sir-release`, and `sona`.

## Important Reduction Observations

- A single branch on an unknown calldata-derived condition did not crash.
- One nested branch did not crash.
- Two nested branches inside an outer unknown branch were needed.
- The first inner branch used the outer unknown condition.
- The second inner branch used a mutable local initialized to `false`.
- `let mut b1 = false` reproduced the issue, but `let b1 = false` did not.
- Reversing the inner branch order avoided the issue.
- The specific source of the unknown boolean did not matter. `@evm_iszero`, `@evm_gt`, `@evm_sgt`, and `@evm_eq(in0, 0)` could all reproduce the same shape.
- `@evm_stop()` was sufficient. Return and memory operations were not relevant.

## Relevant SIR Shape

The minimized Plank program lowered into a control-flow shape like this before optimization:

```sir
fn init:
    bb0 {
        v0 = const 0
        v1 = calldataload v0
        v2 = copy v1
        v3 = iszero v2
        v4 = copy v3
        => v4 ? @bb1 : @bb8
    }
    bb8 {
        => @bb9
    }
    bb1 {
        v5 = const 0
        v6 = copy v3
        => v6 ? @bb2 : @bb3
    }
    bb9 {
        stop
    }
    bb3 {
        => @bb4
    }
    bb2 {
        => @bb4
    }
    bb4 {
        v7 = copy v5
        => v7 ? @bb5 : @bb6
    }
    bb6 {
        => @bb7
    }
    bb5 {
        => @bb7
    }
    bb7 {
        => @bb9
    }
```

The key point is that SCCP can mark a block reachable before every value used by that block's control flow has finished propagating through the value worklist.

## Root Cause

`SCCP::is_edge_reachable` handled branch conditions like this:

```rust
either => {
    debug_assert!(either != LatticeValue::Unknown);
    to == zero_target || to == non_zero_target
}
```

This was internally inconsistent. In non-debug builds, the fallback branch already treated unresolved conditions conservatively by allowing both successor edges. In debug-assertion builds, the `debug_assert!` converted the same valid transient state into a panic.

For SCCP, `Unknown` means "not enough information yet." It does not mean "invalid state." While the analysis is still draining `cfg_worklist` and `values_worklist`, a reachable block can temporarily observe `Unknown` for a control value. The sound response is to keep all possible outgoing edges reachable until the condition becomes more precise.

## Source Code Fix

The branch reachability fallback now explicitly includes `Unknown`, non-proven `EvmConst`, and `Overdefined` values. These all select the conservative behavior: both branch edges remain feasible unless SCCP has proven one specific edge.

Applied fix:

```rust
ControlView::Branches { condition, zero_target, non_zero_target } => {
    match self.lattice[condition] {
        LatticeValue::Const(cv) => {
            if cv.is_zero() {
                to == zero_target
            } else {
                to == non_zero_target
            }
        }
        // Some more constants may be guaranteed non-zero (e.g. number,
        // runtime_start_offset), not including conservatively.
        LatticeValue::EvmConst(
            EvmConstKind::Address
            | EvmConstKind::Origin
            | EvmConstKind::Caller
            | EvmConstKind::Timestamp
            | EvmConstKind::GasLimit
            | EvmConstKind::ChainId,
        ) => to == non_zero_target,
        LatticeValue::Unknown
        | LatticeValue::EvmConst(_)
        | LatticeValue::Overdefined => {
            // A reachable block can observe Unknown while SCCP is still draining its
            // worklists. Values not proven to select one edge keep both edges feasible.
            to == zero_target || to == non_zero_target
        }
    }
}
```

## Regression Test Added

A regression test was added to model the minimized shape directly in SIR:

```rust
#[test]
fn test_unknown_branch_condition_is_conservative_during_worklist_propagation() {
    let input = r#"
        fn init:
            entry {
                zero = const 0
                word = calldataload zero
                cond = iszero word
                cond_copy = copy cond
                => cond_copy ? @outer_true : @outer_false
            }
            outer_false {
                => @done
            }
            outer_true {
                local_false = const 0
                cond_again = copy cond
                => cond_again ? @inner_true : @inner_false
            }
            inner_false {
                => @merge
            }
            inner_true {
                => @merge
            }
            merge {
                later_cond = copy local_false
                => later_cond ? @later_true : @later_false
            }
            later_false {
                => @done
            }
            later_true {
                => @done
            }
            done {
                stop
            }
    "#;

    let mut ir = parse_or_panic(input, EmitConfig::init_only());
    let store = AnalysesStore::default();
    run_pass(&mut SCCP::default(), &mut ir, &store);

    let reachability = store.reachability(&ir);
    assert!(reachability.contains(BasicBlockId::new(5)));
    assert!(reachability.contains(BasicBlockId::new(6)));
    assert!(reachability.contains(BasicBlockId::new(7)));
}
```

## Verification

The following checks pass after the fix:

```sh
cd /workspace/plankc
cargo test -p sir-passes
cargo run -q -p plank -- build /tmp/min22.plk --backend sir-release -Ocsud
```

The original fuzz artifact also replays cleanly:

```sh
cd /workspace/plankc/rappie
cargo +nightly fuzz run plank_backend_program_diff \
  fuzz/artifacts/plank_backend_program_diff/crash-b7d7fe660f405ddc8995ea7b2e55956cd9696e6c
```

Additional project-specific check:

```sh
cd /workspace/plankc/rappie
cargo test -p rappie
```

Observed passing results:

- `sir-passes`: 117 tests passed.
- `rappie`: 21 tests passed.
- Original fuzz artifact executed without panic.
- Minimized Plank repro compiled successfully under `sir-release -Ocsud`.

## Impact

This is a compiler robustness bug in the optimized SIR backend path. It is not evidence of incorrect generated bytecode by itself, because the failure happened before code generation completed. The fix aligns debug-assertion behavior with the conservative behavior that release builds already used for unresolved branch conditions.

The semantic risk of the fix is low:

- It does not make SCCP more aggressive.
- It does not remove any proven constant folding.
- It preserves existing special handling for EVM constants known to be non-zero.
- It only prevents a valid transient `Unknown` lattice value from being treated as an impossible state.

## Suggested Follow-Up

- Commit the SCCP fix and regression test.
- Keep the original fuzz artifact until the fix is committed and validated in CI.
- Consider adding a short comment to SCCP's worklist loop explaining that reachable control users may be processed before all value users have stabilized.
- Continue fuzzing `sir-debug` vs optimized `sir-release` because this finding shows the backend-diff target is successfully exercising optimizer-only behavior.
