use crate::generator::{
    ast::{
        BoolBinaryOp, BoolExpr, CompareOp, MAX_INPUT_WORDS, MIN_INPUT_WORDS, Program, Stmt,
        U256BinaryOp, U256Expr, U256UnaryOp,
    },
    context::GenerationContext,
};
use arbitrary::{Arbitrary, Unstructured};

const MAX_STATEMENTS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedCase {
    program: Program,
    calldata_words: Vec<u64>,
}

impl GeneratedCase {
    pub(crate) fn program(&self) -> &Program {
        &self.program
    }

    pub(crate) fn calldata_words(&self) -> &[u64] {
        &self.calldata_words
    }
}

impl<'a> Arbitrary<'a> for GeneratedCase {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let input_words = u.int_in_range(MIN_INPUT_WORDS..=MAX_INPUT_WORDS)?;
        let calldata_words =
            (0..input_words).map(|_| u64::arbitrary(u)).collect::<arbitrary::Result<Vec<_>>>()?;

        let mut ctx = GenerationContext::new(input_words);
        let mut stmts = Vec::new();
        let statement_count = u.int_in_range(0..=MAX_STATEMENTS)?;

        for _ in 0..statement_count {
            if should_generate_u256_stmt(u)? {
                let expr = arbitrary_u256_expr(u, &ctx)?;
                let id = ctx.allocate_u256();
                stmts.push(Stmt::LetU256 { id, expr });
            } else {
                let expr = arbitrary_bool_expr(u, &ctx)?;
                let id = ctx.allocate_bool();
                stmts.push(Stmt::LetBool { id, expr });
            }
        }

        let result = ctx.choose_u256_value(u)?;
        let program = Program::new(input_words, stmts, result);
        debug_assert!(program.validate().is_ok());

        Ok(Self { program, calldata_words })
    }
}

fn should_generate_u256_stmt(u: &mut Unstructured<'_>) -> arbitrary::Result<bool> {
    Ok(u.int_in_range(0..=4)? <= 2)
}

fn arbitrary_u256_expr(
    u: &mut Unstructured<'_>,
    ctx: &GenerationContext,
) -> arbitrary::Result<U256Expr> {
    let max_choice = if ctx.has_bool_values() { 4 } else { 3 };

    match u.int_in_range(0..=max_choice)? {
        0 => Ok(U256Expr::Const(arbitrary_u256_const(u)?)),
        1 => Ok(U256Expr::Value(ctx.choose_u256_value(u)?)),
        2 => Ok(U256Expr::Unary { op: U256UnaryOp::Not, value: ctx.choose_u256_value(u)? }),
        3 => Ok(U256Expr::Binary {
            op: arbitrary_u256_binary_op(u)?,
            left: ctx.choose_u256_value(u)?,
            right: ctx.choose_u256_value(u)?,
        }),
        4 => Ok(U256Expr::If {
            cond: ctx.choose_bool_value(u)?,
            then_value: ctx.choose_u256_value(u)?,
            else_value: ctx.choose_u256_value(u)?,
        }),
        _ => unreachable!("u256 expression choice is clamped to valid alternatives"),
    }
}

fn arbitrary_bool_expr(
    u: &mut Unstructured<'_>,
    ctx: &GenerationContext,
) -> arbitrary::Result<BoolExpr> {
    let max_choice = if ctx.has_bool_values() { 4 } else { 2 };

    match u.int_in_range(0..=max_choice)? {
        0 => Ok(BoolExpr::Const(bool::arbitrary(u)?)),
        1 => Ok(BoolExpr::IsZero(ctx.choose_u256_value(u)?)),
        2 => Ok(BoolExpr::Compare {
            op: arbitrary_compare_op(u)?,
            left: ctx.choose_u256_value(u)?,
            right: ctx.choose_u256_value(u)?,
        }),
        3 => Ok(BoolExpr::Value(ctx.choose_bool_value(u)?)),
        4 => Ok(BoolExpr::Binary {
            op: arbitrary_bool_binary_op(u)?,
            left: ctx.choose_bool_value(u)?,
            right: ctx.choose_bool_value(u)?,
        }),
        _ => unreachable!("bool expression choice is clamped to valid alternatives"),
    }
}

fn arbitrary_u256_const(u: &mut Unstructured<'_>) -> arbitrary::Result<u64> {
    match u.int_in_range(0..=8)? {
        0 => Ok(0),
        1 => Ok(1),
        2 => Ok(31),
        3 => Ok(32),
        4 => Ok(255),
        5 => Ok(u64::MAX),
        6 => Ok(u64::from(u8::arbitrary(u)?)),
        7 => Ok(u64::from(u16::arbitrary(u)?)),
        8 => u64::arbitrary(u),
        _ => unreachable!("const choice is clamped to valid alternatives"),
    }
}

fn arbitrary_u256_binary_op(u: &mut Unstructured<'_>) -> arbitrary::Result<U256BinaryOp> {
    match u.int_in_range(0..=5)? {
        0 => Ok(U256BinaryOp::Add),
        1 => Ok(U256BinaryOp::Sub),
        2 => Ok(U256BinaryOp::Mul),
        3 => Ok(U256BinaryOp::And),
        4 => Ok(U256BinaryOp::Or),
        5 => Ok(U256BinaryOp::Xor),
        _ => unreachable!("binary op choice is clamped to valid alternatives"),
    }
}

fn arbitrary_compare_op(u: &mut Unstructured<'_>) -> arbitrary::Result<CompareOp> {
    match u.int_in_range(0..=2)? {
        0 => Ok(CompareOp::Eq),
        1 => Ok(CompareOp::Lt),
        2 => Ok(CompareOp::Gt),
        _ => unreachable!("compare op choice is clamped to valid alternatives"),
    }
}

fn arbitrary_bool_binary_op(u: &mut Unstructured<'_>) -> arbitrary::Result<BoolBinaryOp> {
    match u.int_in_range(0..=3)? {
        0 => Ok(BoolBinaryOp::And),
        1 => Ok(BoolBinaryOp::Or),
        2 => Ok(BoolBinaryOp::Xor),
        3 => Ok(BoolBinaryOp::Eq),
        _ => unreachable!("bool binary op choice is clamped to valid alternatives"),
    }
}

#[cfg(test)]
mod tests {
    use super::GeneratedCase;
    use crate::generator::{encode_calldata_words, render_program};
    use arbitrary::{Arbitrary, Unstructured};

    #[test]
    fn generated_cases_are_valid_and_calldata_sized() {
        for fill in [0, 1, 7, 19, 255] {
            let bytes = vec![fill; 4096];
            let mut u = Unstructured::new(&bytes);
            let case = GeneratedCase::arbitrary(&mut u).expect("case should decode");

            case.program().validate().expect("generated program should be valid");
            assert_eq!(
                encode_calldata_words(case.calldata_words()).len(),
                usize::from(case.program().input_words) * 32
            );
        }
    }

    #[test]
    fn fixed_bytes_render_stably() {
        let bytes = vec![42; 4096];

        let mut first = Unstructured::new(&bytes);
        let first_case = GeneratedCase::arbitrary(&mut first).expect("case should decode");
        let first_source = render_program(first_case.program());

        let mut second = Unstructured::new(&bytes);
        let second_case = GeneratedCase::arbitrary(&mut second).expect("case should decode");
        let second_source = render_program(second_case.program());

        assert_eq!(first_source, second_source);
    }
}
