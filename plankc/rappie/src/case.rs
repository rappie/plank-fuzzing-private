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
            ArgValue, Block, BoolExpr, BoolRef, BoolValue, CompareOp, Function, FunctionBody,
            FunctionRef, ParamRef, Program, Stmt, U256BinaryOp, U256Const, U256Expr, U256Ref,
            U256TernaryOp, U256Value, ValueType, render_program,
        },
        oracle::BackendKind,
    };
    use arbitrary::{Arbitrary, Unstructured};

    #[test]
    fn fuzz_case_exposes_source_and_matching_calldata() {
        let bytes = vec![11; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = FuzzCase::arbitrary(&mut u).expect("case should decode");

        assert!(case.source().contains("init {"));
        assert!(case.source().contains("@evm_return(out, 32);"));
        assert_eq!(case.calldata().len() % 32, 0);
    }

    #[test]
    fn hand_constructed_generated_program_compiles() {
        let program = Program::with_functions(
            2,
            vec![Function {
                id: FunctionRef::new(0),
                params: vec![ValueType::U256, ValueType::Bool],
                return_type: ValueType::U256,
                body: FunctionBody::U256(Block {
                    stmts: vec![Stmt::LetU256 {
                        id: U256Ref::new(0),
                        mutable: false,
                        expr: U256Expr::If {
                            cond: BoolValue::Param(ParamRef::new(1)),
                            then_block: Block {
                                stmts: vec![Stmt::LetU256 {
                                    id: U256Ref::new(1),
                                    mutable: false,
                                    expr: U256Expr::Ternary {
                                        op: U256TernaryOp::AddMod,
                                        first: U256Value::Param(ParamRef::new(0)),
                                        second: U256Value::Param(ParamRef::new(0)),
                                        third: U256Value::Param(ParamRef::new(0)),
                                    },
                                }],
                                result: U256Value::Local(U256Ref::new(1)),
                            },
                            else_block: Block {
                                stmts: Vec::new(),
                                result: U256Value::Param(ParamRef::new(0)),
                            },
                        },
                    }],
                    result: U256Value::Local(U256Ref::new(0)),
                }),
            }],
            vec![
                Stmt::LetU256 {
                    id: U256Ref::new(0),
                    mutable: true,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Add,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetBool {
                    id: BoolRef::new(0),
                    mutable: false,
                    expr: BoolExpr::Compare {
                        op: CompareOp::SLt,
                        left: U256Value::Local(U256Ref::new(0)),
                        right: U256Value::Input(0),
                    },
                },
                Stmt::AssignU256 {
                    target: U256Ref::new(0),
                    expr: U256Expr::Block(Block {
                        stmts: vec![Stmt::LetU256 {
                            id: U256Ref::new(1),
                            mutable: false,
                            expr: U256Expr::Const(U256Const::max()),
                        }],
                        result: U256Value::Local(U256Ref::new(1)),
                    }),
                },
                Stmt::LetU256 {
                    id: U256Ref::new(2),
                    mutable: false,
                    expr: U256Expr::Call {
                        function: FunctionRef::new(0),
                        args: vec![
                            ArgValue::U256(U256Value::Local(U256Ref::new(0))),
                            ArgValue::Bool(BoolValue::Local(BoolRef::new(0))),
                        ],
                    },
                },
            ],
            U256Value::Local(U256Ref::new(2)),
        );
        let source = render_program(&program);

        compile_plank_source(&source, BackendKind::SirDebug, None).expect("source should compile");
    }

    #[test]
    fn expanded_builtin_program_compiles() {
        let program = Program::new(
            2,
            vec![
                Stmt::LetU256 {
                    id: U256Ref::new(0),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Div,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(1),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Mod,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(2),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Shl,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(3),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Shr,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(4),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Sar,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(5),
                    mutable: false,
                    expr: U256Expr::Binary {
                        op: U256BinaryOp::Byte,
                        left: U256Value::Input(0),
                        right: U256Value::Input(1),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(6),
                    mutable: false,
                    expr: U256Expr::Ternary {
                        op: U256TernaryOp::MulMod,
                        first: U256Value::Local(U256Ref::new(0)),
                        second: U256Value::Local(U256Ref::new(1)),
                        third: U256Value::Input(1),
                    },
                },
                Stmt::LetBool {
                    id: BoolRef::new(0),
                    mutable: false,
                    expr: BoolExpr::Compare {
                        op: CompareOp::SGt,
                        left: U256Value::Local(U256Ref::new(6)),
                        right: U256Value::Input(0),
                    },
                },
            ],
            U256Value::Local(U256Ref::new(6)),
        );
        let source = render_program(&program);

        compile_plank_source(&source, BackendKind::SirDebug, None).expect("source should compile");
    }
}
