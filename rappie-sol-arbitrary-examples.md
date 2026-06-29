# rappie-sol Arbitrary Case Examples

`plankc/rappie-sol` decodes fuzzer bytes into a structured `FuzzCase`.
Internally that wraps a `GeneratedCase`, which is then rendered into equivalent
Plank and Solidity/Yul sources.

The Rust snippets below are concrete, Rust-like sketches of the private
generator structures in `src/generator/mod.rs`. They are not public constructors,
but they show the shape that `Arbitrary` is producing before rendering.

## Example 1: raw fallback, arithmetic, memory, branch, return

### Arbitrary structure

```rust
GeneratedCase {
    mode: ProgramMode::RawFallback,
    entries: vec![
        Entry {
            selector: 0,
            config: EntryConfig {
                fragments: vec![
                    Fragment::Arithmetic {
                        op: ArithmeticOp::Add,
                        lhs: 0,
                        rhs: 5,
                        aux: 0,
                    },
                    Fragment::Memory {
                        width: 1,
                        offset: 64,
                        value: 6,
                    },
                    Fragment::Branch {
                        left: 5,
                        right: 6,
                    },
                ],
                exit: ExitConfig {
                    kind: ExitKind::Return,
                    output_len: 32,
                },
                constants: ConstantPool {
                    words: [[0; 32]; 4],
                },
            },
        },
    ],
    calls: vec![
        CallStep {
            selected_entry: 0,
            payload: vec![0xde, 0xad, 0xbe, 0xef],
        },
    ],
    frontend: FrontendPlan::default(),
}
```

The call data sequence produced for the oracle is:

```text
call 0: 0xdeadbeef
```

### Rendered Plank

```plank
const entry_0 = fn () never {
    let scratch = @malloc_zeroed(2048);
    let mut acc = @evm_xor(@evm_calldataload(0), 0x0000000000000000000000000000000000000000000000000000000000000000);
    acc = @evm_add(acc, 0x1);
    @mstore1(scratch +% 64, @evm_add(acc, 0xff));
    acc = @evm_xor(acc, @mload1(scratch +% 64));
    if @evm_iszero(@evm_and(acc, 1)) {
        acc = @evm_add(acc, 0x1);
    } else {
        acc = @evm_xor(acc, 0xff);
    }
    @mstore32(scratch, @evm_add(acc, 0xffff));
    @evm_return(scratch, 32);
};

init {
    entry_0();
}
```

### Rendered Solidity/Yul

```solidity
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

contract C {
    fallback() external payable {
        assembly ("memory-safe") {
            function entry_0() {
                let scratch := mload(0x40)
                mstore(0x40, add(scratch, 2048))
                let acc := xor(calldataload(0), 0x0000000000000000000000000000000000000000000000000000000000000000)
                acc := add(acc, 0x1)
                mstore8(add(scratch, 64), add(acc, 0xff))
                acc := xor(acc, byte(0, mload(add(scratch, 64))))
                switch iszero(and(acc, 1))
                case 0 { acc := xor(acc, 0xff) }
                default { acc := add(acc, 0x1) }
                mstore(scratch, add(acc, 0xffff))
                return(scratch, 32)
            }

            entry_0()
        }
    }
}
```

## Example 2: selector dispatch with compound frontend features

This example enables `has_struct` and `has_tuple` in the frontend feature plan.
The Plank side gets generated compound type definitions and helper functions.
The Solidity side mirrors their observable effect directly in Yul.

### Arbitrary structure

```rust
GeneratedCase {
    mode: ProgramMode::SelectorDispatch,
    entries: vec![
        Entry {
            selector: 0x12345678,
            config: EntryConfig {
                fragments: vec![
                    Fragment::Calldata {
                        op: CalldataOp::Load,
                        offset: 4,
                        len: 0,
                        dst: 0,
                    },
                    Fragment::Arithmetic {
                        op: ArithmeticOp::Xor,
                        lhs: 0,
                        rhs: 6,
                        aux: 0,
                    },
                    Fragment::Loop {
                        iterations: 2,
                        value: 5,
                    },
                ],
                exit: ExitConfig {
                    kind: ExitKind::Return,
                    output_len: 0,
                },
                constants: ConstantPool {
                    words: [[0; 32]; 4],
                },
            },
        },
        Entry {
            selector: 0x90abcdef,
            config: EntryConfig {
                fragments: vec![
                    Fragment::Storage {
                        slot: 0,
                        repeated: false,
                    },
                    Fragment::Arithmetic {
                        op: ArithmeticOp::Sub,
                        lhs: 0,
                        rhs: 5,
                        aux: 0,
                    },
                    Fragment::MemoryCopy {
                        dst: 32,
                        src: 0,
                        len: 16,
                    },
                ],
                exit: ExitConfig {
                    kind: ExitKind::Revert,
                    output_len: 0,
                },
                constants: ConstantPool {
                    words: [[0; 32]; 4],
                },
            },
        },
    ],
    calls: vec![
        CallStep {
            selected_entry: 0,
            payload: vec![0xaa, 0xbb, 0xcc],
        },
        CallStep {
            selected_entry: 1,
            payload: vec![0x01, 0x02],
        },
    ],
    frontend: FrontendPlan {
        has_struct: true,
        has_tuple: true,
        ..Default::default()
    },
}
```

The selector dispatch mode prefixes each call payload with the chosen entry
selector:

```text
call 0: 0x12345678aabbcc
call 1: 0x90abcdef0102
```

### Rendered Plank

```plank
const Pair = struct { a: u256, b: u256 };
const Triple = tuple { u256, u256, u256 };
const Numeric = struct 42 { a: u256 };

const mix_pair = fn (p: Pair) u256 {
    let a = p.a;
    let b = @get_field(p, 1);
    return a +% @evm_xor(b, 0x44);
};

const make_triple = fn (x: u256, y: u256) Triple {
    return (x, y, x +% y);
};

const mix_triple = fn (t: Triple) u256 {
    let t2 = @set_field(t, 1, @evm_xor(@get_field(t, 1), 0x55));
    return @get_field(t2, 0) +% @get_field(t2, 1) +% @get_field(t2, 2);
};

const SELECTOR_0 = 0x12345678;
const SELECTOR_1 = 0x90abcdef;

const entry_0 = fn () never {
    let scratch = @malloc_zeroed(2048);
    let mut acc = @evm_xor(@evm_calldataload(4), 0x0000000000000000000000000000000000000000000000000000000000000000);
    let front_pair_0 = Pair { a: acc, b: @evm_xor(acc, 0x1234) };
    acc = @evm_xor(acc, mix_pair(front_pair_0));
    let front_tuple_0 = make_triple(acc, @evm_xor(acc, 0x22));
    acc = @evm_xor(acc, mix_triple(front_tuple_0));
    acc = @evm_xor(acc, @evm_calldataload(4));
    acc = @evm_xor(acc, 0xff);
    let mut i = 0;
    while @evm_lt(i, 2) {
        acc = @evm_add(acc, @evm_xor(i, 0x1));
        @mstore32(scratch +% 1088, acc);
        acc = @evm_xor(acc, @mload32(scratch +% 1088));
        i = @evm_add(i, 1);
    }
    @evm_return(scratch, 0);
};

const entry_1 = fn () never {
    let scratch = @malloc_zeroed(2048);
    let mut acc = @evm_xor(@evm_calldataload(4), 0x0000000000000000000000000000000000000000000000000000000000000000);
    let front_pair_1 = Pair { a: acc, b: @evm_xor(acc, 0x1234) };
    acc = @evm_xor(acc, mix_pair(front_pair_1));
    let front_tuple_1 = make_triple(acc, @evm_xor(acc, 0x22));
    acc = @evm_xor(acc, mix_triple(front_tuple_1));
    acc = @evm_xor(acc, @evm_sload(0x1010000));
    @evm_sstore(0x1010000, @evm_xor(acc, 0x0000000000000000000000000000000000000000000000000000000000000000));
    acc = @evm_add(acc, @evm_sload(0x1010000));
    acc = @evm_sub(acc, 0x1);
    @mcopy(scratch +% 32, scratch, 16);
    acc = @evm_xor(acc, @evm_keccak256(scratch +% 32, 16));
    @evm_revert(scratch, 0);
};

init {
    let selector = @evm_shr(224, @evm_calldataload(0));
    if @evm_eq(selector, SELECTOR_0) {
        entry_0();
    } else {
        if @evm_eq(selector, SELECTOR_1) {
            entry_1();
        } else {
            @evm_revert(@malloc_uninit(0), 0);
        }
    }
}
```

### Rendered Solidity/Yul

```solidity
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

contract C {
    fallback() external payable {
        assembly ("memory-safe") {
            function entry_0() {
                let scratch := mload(0x40)
                mstore(0x40, add(scratch, 2048))
                let acc := xor(calldataload(4), 0x0000000000000000000000000000000000000000000000000000000000000000)
                {
                    let front_a := acc
                    let front_b := xor(acc, 0x1234)
                    let front_mix := add(front_a, xor(front_b, 0x44))
                    acc := xor(acc, front_mix)
                }
                {
                    let t0 := acc
                    let t1 := xor(acc, 0x22)
                    let t2 := add(t0, t1)
                    let t1b := xor(t1, 0x55)
                    let tuple_mix := add(add(t0, t1b), t2)
                    acc := xor(acc, tuple_mix)
                }
                acc := xor(acc, calldataload(4))
                acc := xor(acc, 0xff)
                for { let i := 0 } lt(i, 2) { i := add(i, 1) } {
                    acc := add(acc, xor(i, 0x1))
                    mstore(add(scratch, 1088), acc)
                    acc := xor(acc, mload(add(scratch, 1088)))
                }
                return(scratch, 0)
            }

            function entry_1() {
                let scratch := mload(0x40)
                mstore(0x40, add(scratch, 2048))
                let acc := xor(calldataload(4), 0x0000000000000000000000000000000000000000000000000000000000000000)
                {
                    let front_a := acc
                    let front_b := xor(acc, 0x1234)
                    let front_mix := add(front_a, xor(front_b, 0x44))
                    acc := xor(acc, front_mix)
                }
                {
                    let t0 := acc
                    let t1 := xor(acc, 0x22)
                    let t2 := add(t0, t1)
                    let t1b := xor(t1, 0x55)
                    let tuple_mix := add(add(t0, t1b), t2)
                    acc := xor(acc, tuple_mix)
                }
                acc := xor(acc, sload(0x1010000))
                sstore(0x1010000, xor(acc, 0x0000000000000000000000000000000000000000000000000000000000000000))
                acc := add(acc, sload(0x1010000))
                acc := sub(acc, 0x1)
                mcopy(add(scratch, 32), scratch, 16)
                acc := xor(acc, keccak256(add(scratch, 32), 16))
                revert(scratch, 0)
            }

            let selector := shr(224, calldataload(0))
            switch selector
            case 0x12345678 { entry_0() }
            case 0x90abcdef { entry_1() }
            default { revert(0, 0) }
        }
    }
}
```
