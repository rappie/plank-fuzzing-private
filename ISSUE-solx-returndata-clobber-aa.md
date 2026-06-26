# Issue: solx Forwards Heap Copy Through Stale Return Data After CREATE2

## Summary

The `plank_sol_program_diff` fuzz target found a Solidity-side codegen bug when
compiling the generated Solidity program with `solx`.

This was not a Plank compiler bug. The generated Plank bytecode and `solc`
bytecode agreed. The mismatch only reproduced when Solidity was compiled with
`solx`.

The bad pattern is:

```text
RETURNDATACOPY heap <- return_data
CREATE2
MCOPY heap <- heap
```

`solx` optimized the final heap-to-heap copy into another return-data copy:

```text
RETURNDATACOPY heap <- return_data
CREATE2
RETURNDATACOPY heap <- return_data
```

That transformation is invalid for EVM. `CREATE2`, like other call-like EVM
instructions, replaces the current return-data buffer. After `CREATE2`, reading
from return-data no longer reads the bytes saved by the earlier
`RETURNDATACOPY`.

The optimized bytecode trapped with an out-of-bounds return-data read, while the
correct bytecode copied the already-saved heap bytes with `MCOPY`.

## Affected Path

- Fuzz target:

```text
/workspace/plankc/rappie-sol/fuzz/fuzz_targets/plank_sol_program_diff.rs
```

- Compiler under test:

```text
solx v0.1.4
```

- Upstream checkout tested:

```text
/tmp/solx-latest
commit 9ca42bd47bdf2761976cf14983401e67c09188b9
LLVM commit c6876bba8dc44ddcdf7757475e9b6bbef5410930
```

- Faulty component:

```text
solx-llvm EVM alias analysis
```

- Source file:

```text
/tmp/solx-latest/solx-llvm/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp
```

- Optimizer pass that consumed the incorrect alias result:

```text
llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp
```

## Original Fuzz Artifact

The crashing input was:

```text
/workspace/plankc/rappie-sol/fuzz/artifacts/plank_sol_program_diff/crash-a74d5f1d9f176cd7806c9684a8a39f314a4b57b2
```

Replay command:

```sh
cd /workspace/plankc/rappie-sol
RAPPIE_SOL_SOLX=/home/vscode/.local/bin/solx \
  cargo +nightly fuzz run plank_sol_program_diff \
  fuzz/artifacts/plank_sol_program_diff/crash-a74d5f1d9f176cd7806c9684a8a39f314a4b57b2 -- -runs=1
```

Observed failure before the fix:

```text
Plank/Solidity mismatch:
call 2 success mismatch:
```

Replaying the same artifact with a patched `solx` exits cleanly:

```text
status=0
***       executed the target code on a fixed set of inputs.
```

## Minimized Solidity Reproducer

```solidity
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

contract C {
    fallback() external payable {
        assembly ("memory-safe") {
            let scratch := mload(0x40)
            mstore(0x40, add(scratch, 2048))
            mstore(add(scratch, 512), 0x1234)

            pop(call(
                100000,
                0x1111111111111111111111111111111111111111,
                0,
                add(scratch, 512),
                25,
                add(scratch, 608),
                49
            ))

            returndatacopy(add(scratch, 704), 0, 18)

            mstore(
                add(scratch, 960),
                0x6001600c60003960016000f30000000000000000000000000000000000000000
            )
            pop(create2(0, add(scratch, 960), 13, 0))

            mcopy(add(scratch, 513), add(scratch, 704), 7)
            stop()
        }
    }
}
```

Compile command:

```sh
/tmp/solx-latest/target/release/solx \
  --bin-runtime \
  --evm-version osaka \
  --metadata-hash none \
  --no-cbor-metadata \
  /tmp/solx-mcopy-after-returndata-repro.sol
```

Before the fix, the runtime bytecode contained no `MCOPY` and had two
`RETURNDATACOPY` instructions:

```text
RETURNDATACOPY
CREATE2
RETURNDATACOPY
```

After the fix, the runtime bytecode contains the expected opcodes:

```text
RETURNDATACOPY
CREATE2
MCOPY
```

## Minimized LLVM IR Reproducer

The Solidity reproducer lowers to this minimal LLVM IR shape:

```llvm
target datalayout = "E-p:256:256-i256:256:256-S256-a:256:256"
target triple = "evm"

declare i256 @llvm.evm.create2(i256, ptr addrspace(1), i256, i256)
declare void @llvm.memcpy.p1.p3.i256(ptr addrspace(1) noalias nocapture writeonly, ptr addrspace(3) noalias nocapture readonly, i256, i1 immarg)
declare void @llvm.memmove.p1.p1.i256(ptr addrspace(1) nocapture writeonly, ptr addrspace(1) nocapture readonly, i256, i1 immarg)

define void @returndata_clobber_prevents_memcpy_forwarding() {
  call void @llvm.memcpy.p1.p3.i256(
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    ptr addrspace(3) null,
    i256 18,
    i1 false)
  %created = call i256 @llvm.evm.create2(
    i256 0,
    ptr addrspace(1) inttoptr (i256 1088 to ptr addrspace(1)),
    i256 13,
    i256 0)
  call void @llvm.memmove.p1.p1.i256(
    ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    i256 7,
    i1 false)
  ret void
}
```

Reproduction command before the fix:

```sh
opt -S -aa-pipeline=evm-aa,basic-aa -passes=memcpyopt repro.ll
```

Before the fix, `memcpyopt` incorrectly rewrote the final heap-to-heap transfer
to:

```llvm
call void @llvm.memcpy.p1.p3.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(3) null,
  i256 7,
  i1 false)
```

After the fix, it remains heap-to-heap:

```llvm
call void @llvm.memcpy.p1.p1.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  i256 7,
  i1 false)
```

The conversion from `memmove.p1.p1` to `memcpy.p1.p1` is safe here because the
source and destination ranges do not overlap. The important property is that
the source address space remains heap address space 1, not return-data address
space 3.

## Root Cause

The Yul frontend lowered source-level `mcopy` correctly as heap-to-heap
`llvm.memmove.p1.p1`.

The invalid rewrite happened later in LLVM `MemCpyOptPass`. The pass saw:

```llvm
memcpy heap <- return_data
create2(...)
memmove heap <- heap
```

and tried to forward through the first copy:

```llvm
memcpy heap <- return_data
create2(...)
memcpy heap <- return_data
```

This optimization is only valid if the copied-from return-data memory does not
change between the two transfers.

`MemCpyOptPass` checks that through alias analysis:

```cpp
if (writtenBetween(MSSA, BAA, MCopyLoc, MSSA->getMemoryAccess(MDep),
                   MSSA->getMemoryAccess(M)))
  return false;
```

`EVMAAResult::getModRefInfo` gave the optimizer the wrong answer for
`llvm.evm.create2` queried against return-data address space. It only refined
the call's heap pointer arguments and storage/transient-storage effects. Since
`create2` has no return-data pointer argument, EVM AA returned `NoModRef` for
return-data.

That is unsound. EVM call-like instructions update the return-data buffer even
though return-data is not passed as an explicit pointer argument.

## Source Code Fix

The minimal fix is to teach EVM alias analysis that call-like EVM intrinsics
modify return-data address space.

Applied fix:

```diff
diff --git a/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp b/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp
index 8fe66da5b904..4ba16764a7ae 100644
--- a/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp
+++ b/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp
@@ -116,7 +116,12 @@ ModRefInfo EVMAAResult::getModRefInfo(const CallBase *Call,
       return ModRefInfo::NoModRef;
     return ModRefInfo::ModRef;
   case Intrinsic::evm_return:
+    if (AS == EVMAS::AS_STORAGE || AS == EVMAS::AS_TSTORAGE)
+      return ModRefInfo::Ref;
+    break;
   case Intrinsic::evm_staticcall:
+    if (AS == EVMAS::AS_RETURN_DATA)
+      return ModRefInfo::Mod;
     if (AS == EVMAS::AS_STORAGE || AS == EVMAS::AS_TSTORAGE)
       return ModRefInfo::Ref;
     break;
@@ -125,6 +130,8 @@ ModRefInfo EVMAAResult::getModRefInfo(const CallBase *Call,
   case Intrinsic::evm_call:
   case Intrinsic::evm_callcode:
   case Intrinsic::evm_delegatecall:
+    if (AS == EVMAS::AS_RETURN_DATA)
+      return ModRefInfo::Mod;
     if (AS == EVMAS::AS_STORAGE || AS == EVMAS::AS_TSTORAGE)
       return ModRefInfo::ModRef;
     break;
```

This keeps the existing storage and transient-storage behavior unchanged, but
prevents optimizations from treating return-data as stable across call-like EVM
instructions.

## Regression Test Added

A focused LLVM IR regression test was added:

```text
/tmp/solx-latest/solx-llvm/llvm/test/CodeGen/EVM/returndata-clobber-aa.ll
```

Test contents:

```llvm
; RUN: opt -S -aa-pipeline=evm-aa,basic-aa -passes=memcpyopt < %s | FileCheck %s

target datalayout = "E-p:256:256-i256:256:256-S256-a:256:256"
target triple = "evm"

declare i256 @llvm.evm.create2(i256, ptr addrspace(1), i256, i256)
declare void @llvm.memcpy.p1.p3.i256(ptr addrspace(1) noalias nocapture writeonly, ptr addrspace(3) noalias nocapture readonly, i256, i1 immarg)
declare void @llvm.memmove.p1.p1.i256(ptr addrspace(1) nocapture writeonly, ptr addrspace(1) nocapture readonly, i256, i1 immarg)

define void @returndata_clobber_prevents_memcpy_forwarding() {
; CHECK-LABEL: @returndata_clobber_prevents_memcpy_forwarding(
; CHECK: call void @llvm.memcpy.p1.p3.i256({{.*}}, i256 18,
; CHECK: call i256 @llvm.evm.create2
; CHECK-NEXT: call void @llvm.memcpy.p1.p1.i256({{.*}}, i256 7,
; CHECK-NOT: call void @llvm.memcpy.p1.p3.i256({{.*}}, i256 7,
  call void @llvm.memcpy.p1.p3.i256(
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    ptr addrspace(3) null,
    i256 18,
    i1 false)
  %created = call i256 @llvm.evm.create2(
    i256 0,
    ptr addrspace(1) inttoptr (i256 1088 to ptr addrspace(1)),
    i256 13,
    i256 0)
  call void @llvm.memmove.p1.p1.i256(
    ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    i256 7,
    i1 false)
  ret void
}
```

## Verification

The following checks pass after the fix:

```sh
cd /tmp/solx-latest
ninja -C target-llvm/build-final opt llc install
cargo build --release --bin solx
```

Regression test:

```sh
/tmp/solx-latest/target-llvm/target-final/bin/opt \
  -S \
  -aa-pipeline=evm-aa,basic-aa \
  -passes=memcpyopt \
  < /tmp/solx-latest/solx-llvm/llvm/test/CodeGen/EVM/returndata-clobber-aa.ll \
  | /usr/lib/llvm-18/bin/FileCheck \
      /tmp/solx-latest/solx-llvm/llvm/test/CodeGen/EVM/returndata-clobber-aa.ll
```

Reduced Solidity reproducer:

```sh
/tmp/solx-latest/target/release/solx \
  --bin-runtime \
  --evm-version osaka \
  --metadata-hash none \
  --no-cbor-metadata \
  /tmp/solx-mcopy-after-returndata-repro.sol
```

Observed opcode sequence after the fix:

```text
len 115
ops [(57, 'RETURNDATACOPY'), (102, 'CREATE2'), (112, 'MCOPY')]
```

Original fuzz artifact replay:

```sh
cd /workspace/plankc/rappie-sol
RAPPIE_SOL_SOLX=/tmp/solx-latest/target/release/solx \
  cargo +nightly fuzz run plank_sol_program_diff \
  fuzz/artifacts/plank_sol_program_diff/crash-a74d5f1d9f176cd7806c9684a8a39f314a4b57b2 -- -runs=1
```

Observed result after the fix:

```text
status=0
***       executed the target code on a fixed set of inputs.
```
