#![no_main]

use libfuzzer_sys::fuzz_target;
use rappie::{FuzzCase, backends_match};

fuzz_target!(|case: FuzzCase| {
    let source = case.source();
    let calldata = case.calldata();

    if let Err(err) = backends_match(&source, &calldata) {
        panic!("backend mismatch:\n{err}\n\ncase: {case:#?}\nsource:\n{source}");
    }
});
