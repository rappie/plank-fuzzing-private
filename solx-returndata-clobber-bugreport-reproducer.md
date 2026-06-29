# solx return-data clobber behavioral reproducer

## Summary

`solx` miscompiles a Solidity fallback that copies return data into heap memory,
executes `CREATE2`, then performs a heap-to-heap `mcopy` from the saved bytes.

The source-level behavior is:

```text
CALL identity precompile
RETURNDATACOPY heap <- return_data
CREATE2
MCOPY heap <- heap
RETURN copied heap bytes
```

The vulnerable `solx` EVM backend incorrectly forwards the final heap-to-heap
copy through the old return-data source:

```text
CALL identity precompile
RETURNDATACOPY heap <- return_data
CREATE2
RETURNDATACOPY heap <- return_data
RETURN copied bytes
```

This is invalid because `CREATE2` clobbers the EVM return-data buffer. After
`CREATE2`, return data no longer refers to the bytes returned by the earlier
identity-precompile call.

The Foundry test below compares behavior:

- The same source compiled by normal `solc` succeeds and returns
  `0x11223344556677`.
- The runtime compiled by vulnerable `solx` reverts, because it reads from the
  clobbered return-data buffer after `CREATE2`.

## Requirements

- Foundry installed.
- A vulnerable `solx` binary available locally.
- FFI enabled for Foundry.

Run the test with:

```sh
SOLX=/path/to/vulnerable/solx forge test --ffi -vvv
```

Expected vulnerable failure:

```text
solx success flag differs from solc
```

With a fixed `solx`, the test should pass.

## Project layout

```text
foundry.toml
src/ReturndataClobber.sol
script/compile-solx-runtime.sh
test/ReturndataClobber.t.sol
```

## foundry.toml

```toml
[profile.default]
src = "src"
test = "test"
out = "out"
libs = ["lib"]
solc_version = "0.8.34"
evm_version = "cancun"
ffi = true
```

## src/ReturndataClobber.sol

```solidity
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

contract ReturndataClobber {
    fallback() external payable {
        assembly ("memory-safe") {
            mstore(0x40, 0x500)

            // Identity precompile: returns the 18 input bytes as return data.
            mstore(0x180, 0x112233445566778899aabbccddeeff0011220000000000000000000000000000)
            pop(call(100000, 0x04, 0, 0x180, 18, 0, 0))

            // Save the current return-data buffer into heap memory.
            returndatacopy(0x200, 0, 18)

            // CREATE2 clobbers the EVM return-data buffer.
            mstore(0x300, 0x6001600c60003960016000f30000000000000000000000000000000000000000)
            pop(create2(0, 0x300, 13, 0))

            // Correct code copies from heap 0x200, not from stale return data.
            mcopy(0x101, 0x200, 7)
            return(0x101, 7)
        }
    }
}
```

## script/compile-solx-runtime.sh

```bash
#!/usr/bin/env bash
set -euo pipefail

SOLX_BIN="${SOLX:-solx}"
SOURCE="${1:-src/ReturndataClobber.sol}"

out="$("$SOLX_BIN" \
  --bin-runtime \
  --evm-version cancun \
  --metadata-hash none \
  --no-cbor-metadata \
  "$SOURCE")"

hex="$(printf '%s\n' "$out" | awk '/^[0-9a-fA-F]+$/{print; exit}')"

if [ -z "$hex" ]; then
  echo "solx did not print runtime bytecode" >&2
  exit 1
fi

printf '0x%s' "$hex"
```

Make it executable:

```sh
chmod +x script/compile-solx-runtime.sh
```

## test/ReturndataClobber.t.sol

```solidity
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

import {ReturndataClobber} from "../src/ReturndataClobber.sol";

interface Vm {
    function ffi(string[] calldata command) external returns (bytes memory);
    function etch(address target, bytes calldata code) external;
}

contract ReturndataClobberTest {
    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function testSolxRuntimeMatchesSolcRuntimeBehavior() public {
        ReturndataClobber solcCompiled = new ReturndataClobber();

        (bool solcOk, bytes memory solcOut) = address(solcCompiled).call("");
        require(solcOk, "solc-compiled runtime reverted");
        require(
            keccak256(solcOut) == keccak256(hex"11223344556677"),
            "unexpected solc output"
        );

        string[] memory command = new string[](3);
        command[0] = "bash";
        command[1] = "script/compile-solx-runtime.sh";
        command[2] = "src/ReturndataClobber.sol";

        bytes memory solxRuntime = vm.ffi(command);

        address solxCompiled = address(0x5150);
        vm.etch(solxCompiled, solxRuntime);

        (bool solxOk, bytes memory solxOut) = solxCompiled.call("");

        require(solxOk == solcOk, "solx success flag differs from solc");
        require(
            keccak256(solxOut) == keccak256(solcOut),
            "solx return data differs from solc"
        );
    }
}
```

## Reproduction steps

```sh
mkdir solx-returndata-clobber-repro
cd solx-returndata-clobber-repro

mkdir -p src test script

# Add the files above.
chmod +x script/compile-solx-runtime.sh

SOLX=/path/to/vulnerable/solx forge test --ffi -vvv
```

## Expected result on vulnerable solx

The test fails because the `solc`-compiled runtime succeeds, while the
`solx`-compiled runtime reverts:

```text
solx success flag differs from solc
```

## Expected result on fixed solx

The test passes. Both runtimes return:

```text
0x11223344556677
```

## Root cause

The source-level `mcopy(0x101, 0x200, 7)` is a heap-to-heap copy from the bytes
saved by the previous `returndatacopy(0x200, 0, 18)`.

The vulnerable backend's EVM alias analysis does not model call-like
instructions such as `CREATE2` as modifying the return-data address space.
`memcpyopt` can therefore incorrectly treat the saved heap bytes as still
equivalent to the old return-data source across `CREATE2`.

That optimization is unsound because the EVM return-data buffer is overwritten
by `CREATE2`. The generated runtime attempts to read 7 bytes from the new
return-data buffer, which is empty in this reproducer, so execution reverts.

