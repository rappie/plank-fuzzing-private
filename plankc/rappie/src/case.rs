use crate::generator::{GeneratedCase, encode_calldata_words, render_program};
use arbitrary::{Arbitrary, Unstructured};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzCase {
    generated: GeneratedCase,
}

impl FuzzCase {
    pub fn source(&self) -> String {
        render_program(self.generated.program())
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
    use crate::{
        compiler::compile_plank_source,
        generator::{
            BoolExpr, BoolRef, BoolValue, CompareOp, Program, Stmt, U256BinaryOp, U256Expr,
            U256Ref, U256Value, render_program,
        },
        oracle::BackendKind,
    };
    use arbitrary::{Arbitrary, Unstructured};

    #[test]
    fn fuzz_case_exposes_source_and_matching_calldata() {
        let bytes = vec![11; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = FuzzCase::arbitrary(&mut u).expect("case should decode");

        assert!(case.source().starts_with("init {"));
        assert!(case.source().contains("@evm_return(out, 32);"));
        assert_eq!(case.calldata().len() % 32, 0);
    }

    #[test]
    fn hand_constructed_generated_program_compiles() {
        let program = Program::new(
            2,
            vec![
                Stmt::LetU256 {
                    id: U256Ref::new(0),
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Add,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetBool {
                    id: BoolRef::new(0),
                    expr: BoolExpr::Compare {
                        op: CompareOp::Lt,
                        left: U256Value::Local(U256Ref::new(0)),
                        right: U256Value::Input(0),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(1),
                    expr: U256Expr::If {
                        cond: BoolValue::Local(BoolRef::new(0)),
                        then_value: U256Value::Local(U256Ref::new(0)),
                        else_value: U256Value::Input(1),
                    },
                },
            ],
            U256Value::Local(U256Ref::new(1)),
        );
        let source = render_program(&program);

        compile_plank_source(&source, BackendKind::SirDebug, None).expect("source should compile");
    }
}
