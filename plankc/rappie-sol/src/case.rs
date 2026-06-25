use crate::generator::{
    GeneratedCase, encode_calldata_words, render_plank_program, render_solidity_program,
};
use arbitrary::{Arbitrary, Unstructured};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzCase {
    generated: GeneratedCase,
}

impl FuzzCase {
    pub fn plank_source(&self) -> String {
        render_plank_program(self.generated.program())
    }

    pub fn solidity_source(&self) -> String {
        render_solidity_program(self.generated.program())
    }

    pub fn calldata(&self) -> Vec<u8> {
        encode_calldata_words(self.generated.calldata_words())
    }
}

impl<'a> Arbitrary<'a> for FuzzCase {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self { generated: GeneratedCase::arbitrary(u)? })
    }
}

#[cfg(test)]
mod tests {
    use super::FuzzCase;
    use arbitrary::{Arbitrary, Unstructured};

    #[test]
    fn fuzz_case_exposes_sources_and_matching_calldata() {
        let bytes = vec![11; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = FuzzCase::arbitrary(&mut u).expect("case should decode");

        assert!(case.plank_source().contains("init {"));
        assert!(case.plank_source().contains("@evm_return(out, 32);"));
        assert!(case.solidity_source().contains("contract C {"));
        assert!(case.solidity_source().contains("return(0, 32)"));
        assert_eq!(case.calldata().len() % 32, 0);
    }
}
