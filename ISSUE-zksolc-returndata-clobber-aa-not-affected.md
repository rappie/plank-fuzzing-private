# Issue: zksolc Does Not Forward Heap Copy Through Stale Return Data After Call

## Summary

The `solx` return-data clobber bug was investigated against `zksolc` because
both compilers use Matter Labs LLVM-derived code.

The original bad EVM pattern was:

```text
RETURNDATACOPY heap <- return_data
CREATE2
MCOPY heap <- heap
```

`solx` incorrectly optimized the final heap-to-heap copy into another
return-data copy:

```text
RETURNDATACOPY heap <- return_data
CREATE2
RETURNDATACOPY heap <- return_data
```

That is invalid for EVM because `CREATE2` replaces the current return-data
buffer.

Current `zksolc` was tested with the same Solidity reproducer and with a direct
EraVM LLVM IR reproducer. In both cases the final 7-byte transfer remained a
heap-to-heap copy across the call-like operation.

The conclusion is:

```text
zksolc v1.5.16 is not affected by this exact bug on its current EraVM path.
```

The shared LLVM fork still contains the unfixed EVM alias-analysis code, so an
EVM-target consumer of that fork can still be affected. The current `zksolc`
binary, however, compiles through the EraVM target, not the EVM target.

## Tested Setup

- zksolc binary:

```text
/home/vscode/.local/bin/zksolc
Solidity compiler for ZKsync v1.5.16
LLVM build 9db2a30bec2c4ca9b0bed22b61112848547d447f
```

- ZKsync Solidity frontend required by zksolc:

```text
/home/vscode/.local/bin/zksync-solc
Version: 0.8.30+commit.89ae86f4.Linux.clang
LLVM: 0.8.30-1.0.2
```

- Installed release assets:

```text
https://github.com/matter-labs/era-compiler-solidity/releases/tag/1.5.16
https://github.com/matter-labs/era-solidity/releases/tag/0.8.30-1.0.2
```

- Source checkouts inspected:

```text
/tmp/era-compiler-solidity
commit 5ba066aea9db99da480a405aa7bbcecdd38ffdde
tag 1.5.16

/tmp/era-compiler-llvm
commit 9db2a30bec2c4ca9b0bed22b61112848547d447f

/tmp/era-compiler-llvm-context
commit 68e4674db2c5030d589d758582e9c12ecf12b8a2
```

## Source Path Findings

The current `era-compiler-llvm` EVM target still has the same return-data
alias-analysis hole as the `solx` issue:

```text
/tmp/era-compiler-llvm/llvm/lib/Target/EVM/EVMAliasAnalysis.cpp
```

The EVM AA implementation still does not report `AS_RETURN_DATA` as modified
for `evm_create`, `evm_create2`, `evm_call`, `evm_callcode`, or
`evm_delegatecall`.

However, current `zksolc` does not drive that EVM backend path. The CLI entry
points call the EraVM compilation functions:

```text
/tmp/era-compiler-solidity/era-compiler-solidity/src/zksolc/main.rs
yul_to_eravm
llvm_ir_to_eravm
eravm_assembly_to_eravm
standard_json_eravm
combined_json_eravm
standard_output_eravm
```

The LLVM context crate initializes and builds with the EraVM target:

```text
/tmp/era-compiler-llvm-context/src/eravm/mod.rs
inkwell::targets::Target::initialize_eravm(...)

/tmp/era-compiler-llvm-context/src/eravm/context/mod.rs
era_compiler_common::Target::EraVM
```

EraVM address space 3 is generic ABI-page memory, not a dedicated EVM
return-data address space:

```text
/tmp/era-compiler-llvm-context/src/eravm/context/address_space.rs

AddressSpace::Generic => 3
```

Return data on the EraVM path is represented through explicit pointer and size
state:

```text
@ptr_return_data
@returndatasize
```

and through call results returned from helpers such as:

```text
@__farcall(...) -> { ptr addrspace(3), i1 }
```

Those helpers are not declared `memory(none)`, so LLVM cannot treat them as
invisible no-op barriers for memory dependence purposes.

## Solidity Reproducer

The original `solx` Solidity reproducer was compiled with `zksolc`:

```text
/tmp/solx-mcopy-after-returndata-repro.sol
```

Command, default zksolc Yul codegen:

```sh
rm -rf /tmp/zksolc-returndata-test
mkdir -p /tmp/zksolc-returndata-test

/home/vscode/.local/bin/zksolc \
  --solc /home/vscode/.local/bin/zksync-solc \
  --bin \
  --asm \
  -O3 \
  --evm-version cancun \
  --metadata-hash none \
  --no-cbor-metadata \
  --debug-output-dir /tmp/zksolc-returndata-test \
  /tmp/solx-mcopy-after-returndata-repro.sol
```

Observed result:

```text
status=0
```

`zksolc` emitted a warning that raw assembly `create`/`create2` does not have
normal EVM deployment semantics on EraVM, but compilation succeeded.

The optimized IR contained the correct final copy:

```llvm
tail call void @llvm.memcpy.p1.p1.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  i256 7,
  i1 false)
```

The important property is:

```text
source address space remains heap address space 1
```

It did not become:

```text
llvm.memcpy.p1.p3.i256(..., i256 7, ...)
```

The same Solidity reproducer was also compiled with explicit EVMLA frontend
codegen:

```sh
rm -rf /tmp/zksolc-returndata-test-evmla
mkdir -p /tmp/zksolc-returndata-test-evmla

/home/vscode/.local/bin/zksolc \
  --solc /home/vscode/.local/bin/zksync-solc \
  --codegen evmla \
  --bin \
  --asm \
  -O3 \
  --evm-version cancun \
  --metadata-hash none \
  --no-cbor-metadata \
  --debug-output-dir /tmp/zksolc-returndata-test-evmla \
  /tmp/solx-mcopy-after-returndata-repro.sol
```

Observed result:

```text
status=0
```

The optimized IR again contained the correct final heap-to-heap transfer:

```llvm
tail call void @llvm.memcpy.p1.p1.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  i256 7,
  i1 false)
```

## Direct EraVM LLVM IR Reproducer

A direct LLVM IR reproducer was also compiled through `zksolc --llvm-ir` to
avoid relying on Solidity frontend details:

```text
/tmp/zksolc-era-returndata-forward.ll
```

Test shape:

```llvm
target datalayout = "E-p:256:256-i256:256:256-S32-a:256:256"
target triple = "eravm-unknown-unknown"

declare void @llvm.memcpy.p1.p3.i256(ptr addrspace(1), ptr addrspace(3), i256, i1 immarg)
declare void @llvm.memmove.p1.p1.i256(ptr addrspace(1), ptr addrspace(1), i256, i1 immarg)
declare { ptr addrspace(3), i1 } @__farcall(i256, i256, i256, i256, i256, i256, i256, i256, i256, i256, i256, i256)

define i256 @__entry(ptr addrspace(3) %retdata, ...) {
entry:
  call void @llvm.memcpy.p1.p3.i256(
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    ptr addrspace(3) %retdata,
    i256 18,
    i1 false)
  %called = call { ptr addrspace(3), i1 } @__farcall(...)
  call void @llvm.memmove.p1.p1.i256(
    ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
    ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
    i256 7,
    i1 false)
  ...
}
```

Compile command:

```sh
rm -rf /tmp/zksolc-era-ir-test
mkdir -p /tmp/zksolc-era-ir-test

/home/vscode/.local/bin/zksolc \
  --llvm-ir \
  -O3 \
  --metadata-hash none \
  --no-cbor-metadata \
  --debug-output-dir /tmp/zksolc-era-ir-test \
  /tmp/zksolc-era-returndata-forward.ll \
  --bin
```

Observed result:

```text
status=0
```

Optimized IR result:

```llvm
tail call void @llvm.memcpy.p1.p3.i256(
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  ptr addrspace(3) %retdata,
  i256 18,
  i1 false)
%called = tail call { ptr addrspace(3), i1 } @__farcall(...)
tail call void @llvm.memcpy.p1.p1.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  i256 7,
  i1 false)
```

The `memmove.p1.p1` was safely converted to `memcpy.p1.p1`, but the source
address space stayed heap address space 1. It was not forwarded back to the
generic address-space 3 source across `__farcall`.

## Control Test

A control test removed the intervening `__farcall`:

```text
/tmp/zksolc-era-returndata-forward-no-call.ll
```

Control shape:

```llvm
memcpy heap <- generic
memmove heap <- heap
```

Compile command:

```sh
rm -rf /tmp/zksolc-era-ir-control
mkdir -p /tmp/zksolc-era-ir-control

/home/vscode/.local/bin/zksolc \
  --llvm-ir \
  -O3 \
  --metadata-hash none \
  --no-cbor-metadata \
  --debug-output-dir /tmp/zksolc-era-ir-control \
  /tmp/zksolc-era-returndata-forward-no-call.ll \
  --bin
```

Observed optimized IR:

```llvm
tail call void @llvm.memcpy.p1.p3.i256(
  ptr addrspace(1) inttoptr (i256 832 to ptr addrspace(1)),
  ptr addrspace(3) %retdata,
  i256 18,
  i1 false)
tail call void @llvm.memcpy.p1.p3.i256(
  ptr addrspace(1) inttoptr (i256 641 to ptr addrspace(1)),
  ptr addrspace(3) %retdata,
  i256 7,
  i1 false)
```

This proves the test is sensitive to the optimization. `MemCpyOpt` does forward
the source when the intervening call is absent, but it does not forward across
the call-like EraVM operation.

## Verification Summary

Exact optimized-IR checks:

```text
/tmp/zksolc-returndata-test/_tmp_solx-mcopy-after-returndata-repro.sol_C.optimized.ll
  exact p1.p1 len7: 1
  exact p1.p3 len7: 0

/tmp/zksolc-returndata-test-evmla/_tmp_solx-mcopy-after-returndata-repro.sol_C.optimized.ll
  exact p1.p1 len7: 1
  exact p1.p3 len7: 0

/tmp/zksolc-era-ir-test/_tmp_zksolc-era-returndata-forward.ll.optimized.ll
  exact p1.p1 len7: 1
  exact p1.p3 len7: 0

/tmp/zksolc-era-ir-control/_tmp_zksolc-era-returndata-forward-no-call.ll.optimized.ll
  exact p1.p1 len7: 0
  exact p1.p3 len7: 1
```

The zksolc assembly output for the direct EraVM IR test also loaded from heap
offset `832` after the `__farcall`, then stored into heap offset `641`:

```text
call r0, @__farcall, @DEFAULT_UNWIND
ldm.h 641, r1
ldm.h 832, r2
stm.h 641, r1
```

This is the expected heap-to-heap behavior.

## Conclusion

`zksolc v1.5.16` is not affected by the exact `solx` return-data clobber bug.

The key difference is target modeling:

- `solx` EVM IR used a dedicated return-data address space and EVM intrinsics
  like `llvm.evm.create2`. The EVM alias analysis failed to mark return-data as
  modified by call-like intrinsics.
- `zksolc` uses the EraVM target. Return data is represented as generic ABI-page
  pointers and explicit call results. The intervening `__farcall` is visible to
  LLVM as a call that may affect memory, so `MemCpyOpt` does not forward a
  generic-memory source across it.

The EVM target in `era-compiler-llvm` should still receive the return-data
ModRef fix if it is used by any compiler. That shared-code issue is separate
from the current `zksolc` EraVM compilation path tested here.
