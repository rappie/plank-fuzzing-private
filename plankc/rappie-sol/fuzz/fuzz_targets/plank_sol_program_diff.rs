#![no_main]

use libfuzzer_sys::fuzz_target;
use rappie_sol::{FuzzCase, compare_plank_solidity};

fuzz_target!(|case: FuzzCase| {
    if let Err(err) = compare_plank_solidity(&case) {
        let plank_source = case.plank_source();
        let solidity_source = case.solidity_source();
        let calldatas = case.calldatas();

        panic!(
            "Plank/Solidity mismatch:\n{err}\n\ncalldatas:\n{}\n\ncase: {case:#?}\n\nPlank source:\n{plank_source}\n\nSolidity source:\n{solidity_source}",
            hex_encode_all(&calldatas)
        );
    }
});

fn hex_encode_all(calldatas: &[Vec<u8>]) -> String {
    calldatas
        .iter()
        .enumerate()
        .map(|(index, calldata)| format!("call {index}: 0x{}", hex_encode(calldata)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}
