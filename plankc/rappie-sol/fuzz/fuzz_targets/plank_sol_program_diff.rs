#![no_main]

use libfuzzer_sys::fuzz_target;
use rappie_sol::{FuzzCase, compare_plank_solidity};

#[unsafe(no_mangle)]
pub extern "C" fn __asan_default_options() -> *const std::ffi::c_char {
    static OPTIONS: &[u8] = b"quarantine_size_mb=16:malloc_context_size=8\0";
    OPTIONS.as_ptr().cast()
}

fuzz_target!(|case: FuzzCase| {
    if let Err(err) = compare_plank_solidity(&case) {
        let plank_sources = case.plank_sources_display();
        let solidity_source = case.solidity_source();
        let calldatas = case.calldatas();

        panic!(
            "Plank/Solidity mismatch:\n{err}\n\ncalldatas:\n{}\n\ncase: {case:#?}\n\nPlank sources:\n{plank_sources}\n\nSolidity source:\n{solidity_source}",
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
