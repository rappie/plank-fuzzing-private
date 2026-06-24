# Issue: SCCP Skips Late Predecessor Outputs for Reachable Merge Blocks

## Summary

The `plank_backend_program_diff` fuzz target found a real backend output mismatch between `sir-debug` and optimized `sir-release -Ocsud`.

This is distinct from the earlier SCCP assertion crash. The previous issue was a debug-only panic on a transient `Unknown` branch condition. This issue is a semantic optimizer bug: SCCP can incorrectly fold a merge-block value to a constant when a later-discovered predecessor edge also feeds that merge.

Observed mismatch:

```text
reference sir-debug:       0x0000000000000000000000000000000000000000000000000000000000000001
candidate sir-release-csud: 0x0000000000000000000000000000000000000000000000000000000000000000
```

The optimized backend returned `0`, but the correct result for the crashing calldata is `1`.

## Affected Path

- Fuzz target: `rappie/fuzz/fuzz_targets/plank_backend_program_diff.rs`
- Compared backends:
  - `sir-debug`
  - `sir-release -Ocsud`
- Faulty pass: `SCCP`
- Faulty function: `SCCP::mark_reachable`
- Source file:

```text
/workspace/plankc/sir/crates/passes/src/optimizations/constant_propagation.rs
```

## Original Fuzz Artifact

The crashing input was:

```text
/workspace/plankc/rappie/fuzz/artifacts/plank_backend_program_diff/crash-e6a8c01a5b67454bc241d5e08e0b8328b31360ae
```

Replay command:

```sh
cd /workspace/plankc/rappie
cargo +nightly fuzz run plank_backend_program_diff \
  fuzz/artifacts/plank_backend_program_diff/crash-e6a8c01a5b67454bc241d5e08e0b8328b31360ae
```

Decoded calldata word:

```text
5073072362328244257
```

Encoded calldata:

```text
00000000000000000000000000000000000000000000000046672c6f6c6d4021
```

## Original Generated Plank Program

```plk
init {
    let in0 = @evm_calldataload(0);

    let v0 = 0x82573746ff2656daa2e2a2e1b1b1b1b1b1b1b01000001000000000006002e;
    let v1 = @evm_addmod(in0, in0, v0);
    let mut v2 = @evm_not(in0);
    let v3 = in0;
    let b0 = @evm_iszero(in0);
    let v4 = @evm_not(in0);
    let b1 = b0;
    v2 = 0x100000000000000000000000000000000;
    if b0 {
    } else {
    }
    let mut b2 = if b0 {
        v2 = 0;
        b0
    } else {
        b0
    };
    let v5 = 0x2e;
    b2 = b0;
    v2 = @evm_div(v2, v2);
    v2 = @evm_div(v2, v2);
    v2 = @evm_div(v2, v2);
    v2 = @evm_div(v2, v2);

    let out = @malloc_uninit(32);
    @mstore32(out, v2);
    @evm_return(out, 32);
}
```

For the decoded calldata, `in0` is nonzero. Therefore `b0 = @evm_iszero(in0)` is false, the `v2 = 0` assignment in the true branch must not execute, and the repeated `v2 / v2` operations should produce `1`.

## Minimized Plank Reproducer

This smaller program captures the same bug:

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

For nonzero calldata, the false branch keeps `v0 = 2`, so the return value should be `2 / 2 = 1`.

Before the fix, optimized SCCP could treat the merge input as constant `0`, fold `v0 / v0` to `0 / 0`, and return `0`.

## Relevant Optimized SIR Before the Fix

The important part of the bad optimized SIR was:

```sir
fn init:
    bb0 {
        v0 = const 0
        v1 = calldataload v0
        v2 = iszero v1
        v3 = large_const 0x100000000000000000000000000000000
        => v2 ? @bb6 : @bb1
    }
    bb1 -> v2 v3 {
        => @bb2
    }
    bb6 -> v2 v3 {
        => @bb2
    }
    bb2 v4 v5 {
        => v4 ? @bb5 : @bb3
    }
    bb3 -> v4 v4 v5 {
        => @bb4
    }
    bb5 -> v4 v4 v13 {
        v13 = const 0
        => @bb4
    }
    bb4 v6 v7 v8 {
        v9 = const 0
        v10 = const 32
        v11 = mallocany v10
        mstore256 v11 v9
        v12 = const 32
        return v11 v12
    }
```

The final merge block had been simplified to return constant `0`, even though the false path can feed a nonzero value.

## Root Cause

`SCCP::mark_reachable` only flowed predecessor block outputs when the successor block was first marked reachable:

```rust
fn mark_reachable(
    &mut self,
    program: &EthIRProgram,
    from: BasicBlockId,
    to: BasicBlockId,
    reachable: &mut DenseIndexSet<BasicBlockId>,
) {
    if !reachable.contains(to) {
        reachable.add(to);
        self.cfg_worklist.push(to);
        self.flow_outputs_to(program, from, to);
    }
}
```

That is unsound for merge blocks. A successor block should only be scheduled once, but every feasible predecessor edge must contribute its outputs to the successor inputs.

In the failing shape, SCCP discovered one edge to the merge first and flowed a constant `0`. Later, another predecessor edge became reachable and should have merged in a different value. Because the merge block was already marked reachable, `flow_outputs_to` did not run for that later edge, leaving the merge input incorrectly constant.

## Source Code Fix

The fix is to separate "edge output flow" from "first-time block scheduling":

```rust
fn mark_reachable(
    &mut self,
    program: &EthIRProgram,
    from: BasicBlockId,
    to: BasicBlockId,
    reachable: &mut DenseIndexSet<BasicBlockId>,
) {
    self.flow_outputs_to(program, from, to);

    if !reachable.contains(to) {
        reachable.add(to);
        self.cfg_worklist.push(to);
    }
}
```

This preserves the existing behavior that each block is only pushed onto the CFG worklist when first marked reachable, while ensuring every reachable predecessor edge contributes to block input lattice values.

## Regression Test Added

A focused SCCP regression test was added:

```rust
#[test]
fn test_late_reachable_predecessor_flows_outputs_to_reachable_merge() {
    let input = r#"
        fn init:
            entry {
                zero = const 0
                word = calldataload zero
                cond = iszero word
                two = const 2
                => cond ? @set_zero : @keep_two
            }
            keep_two -> two {
                => @merge
            }
            set_zero -> zero_value {
                zero_value = const 0
                => @merge
            }
            merge value {
                result = div value value
                stop
            }
    "#;

    let (_, sccp) = run_const_prop(input);

    assert_eq!(sccp.lattice[LocalId::new(5)], LatticeValue::Overdefined);
    assert_eq!(sccp.lattice[LocalId::new(6)], LatticeValue::Overdefined);
}
```

The test requires the merge input and the computed division result to become `Overdefined`, not constant `0`.

## Verification

The following checks pass after the fix:

```sh
cd /workspace/plankc
cargo test -p sir-passes
```

Observed result:

```text
118 passed; 0 failed
```

Project-specific harness checks:

```sh
cd /workspace/plankc/rappie
cargo test -p rappie
```

Observed result:

```text
21 passed; 0 failed
```

The new fuzz artifact replays cleanly:

```sh
cd /workspace/plankc/rappie
cargo +nightly fuzz run plank_backend_program_diff \
  fuzz/artifacts/plank_backend_program_diff/crash-e6a8c01a5b67454bc241d5e08e0b8328b31360ae
```

The previous SCCP assertion artifact also still replays cleanly:

```sh
cd /workspace/plankc/rappie
cargo +nightly fuzz run plank_backend_program_diff \
  fuzz/artifacts/plank_backend_program_diff/crash-b7d7fe660f405ddc8995ea7b2e55956cd9696e6c
```

## Impact

This is a correctness bug in SCCP, not just a debug-mode crash. It can cause optimized SIR release output to differ from unoptimized SIR debug output for valid Plank programs.

The fix is low-risk and local:

- It does not make SCCP more aggressive.
- It preserves one-time CFG scheduling for newly reachable blocks.
- It corrects merge-input lattice propagation by processing every feasible predecessor edge.
- It prevents constants from being retained when a later reachable predecessor should make the value `Overdefined`.

## Suggested Follow-Up

- File this as a GitHub issue with the generated Plank program, minimized Plank reproducer, and the source-level fix.
- Keep the artifact in the local fuzz corpus until upstream has a fix.
- Continue fuzzing with this local SCCP fix applied, because otherwise the fuzzer will keep rediscovering this same optimizer mismatch.
- Consider adding more SCCP tests for multi-predecessor merge blocks where predecessors become reachable in different worklist orders.
