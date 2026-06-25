use crate::generator::GeneratedCase;
use arbitrary::{Arbitrary, Unstructured};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzCase {
    generated: GeneratedCase,
}

impl FuzzCase {
    pub fn plank_source(&self) -> String {
        self.generated.plank_source()
    }

    pub fn solidity_source(&self) -> String {
        self.generated.solidity_source()
    }

    pub fn calldata(&self) -> Vec<u8> {
        self.generated.calldata()
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
        assert!(case.plank_source().contains("@evm_sstore"));
        assert!(
            case.plank_source().contains("@evm_return")
                || case.plank_source().contains("@evm_revert")
        );
        assert!(case.solidity_source().contains("contract C {"));
        assert!(case.solidity_source().contains("sstore("));

        let calldata = case.calldata();
        assert!(
            calldata.len() % 32 == 0 || (calldata.len() >= 4 && (calldata.len() - 4) % 32 == 0)
        );
    }
}
