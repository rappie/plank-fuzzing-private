#![no_main]

use libfuzzer_sys::fuzz_target;
use rappie::{FuzzCase, compare_default_backends};

fuzz_target!(|case: FuzzCase| {
    let source = case.source();
    let calldata = case.calldata();

    if let Err(err) = compare_default_backends(&source, &calldata) {
        panic!("backend mismatch:\n{err}\n\ncase: {case:#?}\nsource:\n{source}");
    }
});
