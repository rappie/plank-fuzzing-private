use alloy_primitives::U256;
use plank_driver as _;
use plank_evm as _;
use plank_source as _;
use rappie::{Expr, assert_backends_match, render_program};
use revm as _;

const SIMPLE_ADD: &str = r#"
init {
    let a = @evm_calldataload(0);
    let b = @evm_calldataload(32);
    let result = a +% b;

    let out = @malloc_uninit(32);
    @mstore32(out, result);
    @evm_return(out, 32);
}
"#;

#[test]
fn sir_debug_and_sir_release_match_for_simple_add() {
    let calldata = calldata_words([U256::from(7), U256::from(3)]);
    assert_backends_match(SIMPLE_ADD, &calldata);
}

#[test]
fn sir_debug_and_sir_release_match_for_generated_expressions() {
    use Expr::{Add, And, CalldataWord0 as A, CalldataWord1 as B, Const, Xor};

    let expressions = [
        Const(0),
        Const(1),
        A,
        B,
        Add(boxed(A), boxed(B)),
        Xor(boxed(A), boxed(B)),
        And(boxed(Xor(boxed(A), boxed(B))), boxed(Const(255))),
        Add(boxed(And(boxed(A), boxed(Const(255)))), boxed(And(boxed(B), boxed(Const(255))))),
    ];

    let calldata = calldata_words([U256::from(7), U256::from(3)]);
    for expr in expressions {
        let source = render_program(&expr);
        assert_backends_match(&source, &calldata);
    }
}

fn calldata_words(words: impl IntoIterator<Item = U256>) -> Vec<u8> {
    let mut calldata = Vec::new();
    for word in words {
        calldata.extend_from_slice(&word.to_be_bytes::<32>());
    }
    calldata
}

fn boxed(expr: Expr) -> Box<Expr> {
    Box::new(expr)
}
