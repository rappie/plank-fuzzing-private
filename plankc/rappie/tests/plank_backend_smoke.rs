use alloy_primitives::U256;
use plank_driver as _;
use plank_evm as _;
use plank_source as _;
use rappie::{BackendKind, assert_same_result, compile_plank_source, run_bytecode};
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
    let sir_debug = compile_plank_source(SIMPLE_ADD, BackendKind::SirDebug)
        .expect("sir-debug compilation should succeed");
    let sir_release = compile_plank_source(SIMPLE_ADD, BackendKind::SirRelease)
        .expect("sir-release compilation should succeed");

    let calldata = calldata_words([U256::from(7), U256::from(3)]);
    let sir_debug_result = run_bytecode(&sir_debug, &calldata);
    let sir_release_result = run_bytecode(&sir_release, &calldata);

    assert_same_result("sir-debug", &sir_debug_result, "sir-release", &sir_release_result);
}

fn calldata_words(words: impl IntoIterator<Item = U256>) -> Vec<u8> {
    let mut calldata = Vec::new();
    for word in words {
        calldata.extend_from_slice(&word.to_be_bytes::<32>());
    }
    calldata
}
