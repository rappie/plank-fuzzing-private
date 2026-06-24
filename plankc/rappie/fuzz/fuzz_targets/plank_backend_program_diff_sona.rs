#![no_main]

use libfuzzer_sys::fuzz_target;
use rappie::{BackendSpec, FuzzCase, SIR_DEBUG, SONA_O0, compare_backend_set};

const SONA_BACKEND_SET: [BackendSpec; 2] = [SIR_DEBUG, SONA_O0];

fuzz_target!(|case: FuzzCase| {
    let source = case.source();
    let calldata = case.calldata();

    if let Err(err) = compare_backend_set(&source, &calldata, &SONA_BACKEND_SET) {
        panic!("backend mismatch:\n{err}\n\ncase: {case:#?}\nsource:\n{source}");
    }
});
