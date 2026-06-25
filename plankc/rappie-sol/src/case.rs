use crate::generator::{GeneratedCase, SeedClassification};
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

    pub fn seed_classification(&self) -> SeedClassification {
        self.generated.seed_classification()
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
    use std::{fs, path::Path};

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

    #[test]
    fn committed_seeds_decode_and_render() {
        let cases = committed_seed_cases();

        assert!(cases.len() >= 40, "expected a curated committed seed corpus");
        for (name, case) in cases {
            let classification = case.seed_classification();
            assert!(!case.plank_source().is_empty(), "{name} rendered empty Plank source");
            assert!(
                case.solidity_source().contains("contract C"),
                "{name} rendered unexpected Solidity source"
            );
            assert!(classification.entry_count >= 1, "{name} has no entries");
        }
    }

    #[test]
    #[ignore = "requires RAPPIE_SOL_SOLC, SOLC_PATH, or solc on PATH"]
    fn committed_seeds_compare_plank_solidity() {
        for (name, case) in committed_seed_cases() {
            crate::compare_plank_solidity(&case)
                .unwrap_or_else(|err| panic!("{name} did not compare successfully: {err}"));
        }
    }

    fn committed_seed_cases() -> Vec<(String, FuzzCase)> {
        let seed_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/plank_sol_program_diff");
        let mut seeds = fs::read_dir(&seed_dir)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", seed_dir.display()))
            .map(|entry| entry.expect("seed entry should be readable").path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        seeds.sort();

        seeds
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path)
                    .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
                let mut unstructured = Unstructured::new(&bytes);
                let case = FuzzCase::arbitrary(&mut unstructured)
                    .unwrap_or_else(|err| panic!("failed to decode {}: {err}", path.display()));
                let name = path
                    .file_name()
                    .expect("seed should have a file name")
                    .to_string_lossy()
                    .into_owned();

                (name, case)
            })
            .collect()
    }
}
