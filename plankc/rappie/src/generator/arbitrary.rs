use crate::generator::{
    ast::{
        Block, BoolBinaryOp, BoolExpr, CompareOp, Function, FunctionBody, FunctionRef,
        FunctionSignature, MAX_INPUT_WORDS, MIN_INPUT_WORDS, Program, Stmt, StmtBlock,
        U256BinaryOp, U256Const, U256Expr, U256TernaryOp, U256UnaryOp, ValueType,
    },
    context::GenerationContext,
};
use arbitrary::{Arbitrary, Unstructured};

const MAX_TOP_LEVEL_STATEMENTS: usize = 32;
const MAX_BLOCK_STATEMENTS: usize = 8;
const MAX_BLOCK_DEPTH: u8 = 2;
const MAX_FUNCTIONS: usize = 4;
const MAX_FUNCTION_PARAMS: usize = 4;

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

        let functions = arbitrary_functions(u)?;
        let signatures = functions.iter().map(Function::signature).collect();
        let mut ctx = GenerationContext::for_init(input_words, signatures);
        let stmts = arbitrary_stmts(u, &mut ctx, MAX_TOP_LEVEL_STATEMENTS, 0)?;
        let result = ctx.choose_u256_value(u)?;

        let program = Program::with_functions(input_words, functions, stmts, result);
        debug_assert!(program.validate().is_ok());

        Ok(Self { program, calldata_words })
    }
}

fn arbitrary_functions(u: &mut Unstructured<'_>) -> arbitrary::Result<Vec<Function>> {
    let function_count = u.int_in_range(0..=MAX_FUNCTIONS)?;
    let mut functions = Vec::with_capacity(function_count);
    let mut visible_signatures = Vec::new();

    for index in 0..function_count {
        let id = FunctionRef::new(index);
        let params = arbitrary_params(u)?;
        let return_type = arbitrary_value_type(u)?;
        let mut ctx = GenerationContext::for_function(params.clone(), visible_signatures.clone());
        let body = match return_type {
            ValueType::U256 => FunctionBody::U256(arbitrary_u256_block(u, &mut ctx, 0)?),
            ValueType::Bool => FunctionBody::Bool(arbitrary_bool_block(u, &mut ctx, 0)?),
        };
        let function = Function { id, params, return_type, body };

        visible_signatures.push(FunctionSignature {
            id,
            params: function.params.clone(),
            return_type,
        });
        functions.push(function);
    }

    Ok(functions)
}

fn arbitrary_params(u: &mut Unstructured<'_>) -> arbitrary::Result<Vec<ValueType>> {
    let param_count = u.int_in_range(0..=MAX_FUNCTION_PARAMS)?;
    (0..param_count).map(|_| arbitrary_value_type(u)).collect()
}

fn arbitrary_value_type(u: &mut Unstructured<'_>) -> arbitrary::Result<ValueType> {
    if bool::arbitrary(u)? { Ok(ValueType::U256) } else { Ok(ValueType::Bool) }
}

fn arbitrary_stmts(
    u: &mut Unstructured<'_>,
    ctx: &mut GenerationContext,
    max_stmts: usize,
    depth: u8,
) -> arbitrary::Result<Vec<Stmt>> {
    let statement_count = u.int_in_range(0..=max_stmts)?;
    let mut stmts = Vec::with_capacity(statement_count);

    for _ in 0..statement_count {
        stmts.push(arbitrary_stmt(u, ctx, depth)?);
    }

    Ok(stmts)
}

fn arbitrary_stmt(
    u: &mut Unstructured<'_>,
    ctx: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<Stmt> {
    let mut choices = vec![StmtChoice::LetU256, StmtChoice::LetBool];

    if ctx.has_mutable_u256_values() {
        choices.push(StmtChoice::AssignU256);
    }
    if ctx.has_mutable_bool_values() {
        choices.push(StmtChoice::AssignBool);
    }
    if ctx.has_bool_values() && depth < MAX_BLOCK_DEPTH {
        choices.push(StmtChoice::If);
    }

    let choice = choices[u.int_in_range(0..=choices.len() - 1)?];
    match choice {
        StmtChoice::LetU256 => {
            let expr = arbitrary_u256_expr(u, ctx, depth)?;
            let mutable = arbitrary_mutability(u)?;
            let id = ctx.allocate_u256(mutable);
            Ok(Stmt::LetU256 { id, mutable, expr })
        }
        StmtChoice::LetBool => {
            let expr = arbitrary_bool_expr(u, ctx, depth)?;
            let mutable = arbitrary_mutability(u)?;
            let id = ctx.allocate_bool(mutable);
            Ok(Stmt::LetBool { id, mutable, expr })
        }
        StmtChoice::AssignU256 => {
            let target = ctx.choose_mutable_u256(u)?;
            let expr = arbitrary_u256_expr(u, ctx, depth)?;
            Ok(Stmt::AssignU256 { target, expr })
        }
        StmtChoice::AssignBool => {
            let target = ctx.choose_mutable_bool(u)?;
            let expr = arbitrary_bool_expr(u, ctx, depth)?;
            Ok(Stmt::AssignBool { target, expr })
        }
        StmtChoice::If => {
            let cond = ctx.choose_bool_value(u)?;
            let then_block = arbitrary_stmt_block(u, ctx, depth + 1)?;
            let else_block = arbitrary_stmt_block(u, ctx, depth + 1)?;
            Ok(Stmt::If { cond, then_block, else_block })
        }
    }
}

fn arbitrary_mutability(u: &mut Unstructured<'_>) -> arbitrary::Result<bool> {
    Ok(u.int_in_range(0..=4)? == 0)
}

fn arbitrary_u256_expr(
    u: &mut Unstructured<'_>,
    ctx: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<U256Expr> {
    let mut choices = vec![U256ExprChoice::Const];

    if ctx.has_u256_values() {
        choices.push(U256ExprChoice::Value);
        choices.push(U256ExprChoice::Unary);
        choices.push(U256ExprChoice::Binary);
        choices.push(U256ExprChoice::Ternary);
    }
    if !ctx.callable_functions_returning(ValueType::U256).is_empty() {
        choices.push(U256ExprChoice::Call);
    }
    if depth < MAX_BLOCK_DEPTH {
        choices.push(U256ExprChoice::Block);
        if ctx.has_bool_values() {
            choices.push(U256ExprChoice::If);
        }
    }

    let choice = choices[u.int_in_range(0..=choices.len() - 1)?];
    match choice {
        U256ExprChoice::Const => Ok(U256Expr::Const(arbitrary_u256_const(u)?)),
        U256ExprChoice::Value => Ok(U256Expr::Value(ctx.choose_u256_value(u)?)),
        U256ExprChoice::Unary => {
            Ok(U256Expr::Unary { op: U256UnaryOp::Not, value: ctx.choose_u256_value(u)? })
        }
        U256ExprChoice::Binary => Ok(U256Expr::Binary {
            op: arbitrary_u256_binary_op(u)?,
            left: ctx.choose_u256_value(u)?,
            right: ctx.choose_u256_value(u)?,
        }),
        U256ExprChoice::Ternary => Ok(U256Expr::Ternary {
            op: arbitrary_u256_ternary_op(u)?,
            first: ctx.choose_u256_value(u)?,
            second: ctx.choose_u256_value(u)?,
            third: ctx.choose_u256_value(u)?,
        }),
        U256ExprChoice::If => Ok(U256Expr::If {
            cond: ctx.choose_bool_value(u)?,
            then_block: arbitrary_u256_block(u, ctx, depth + 1)?,
            else_block: arbitrary_u256_block(u, ctx, depth + 1)?,
        }),
        U256ExprChoice::Block => Ok(U256Expr::Block(arbitrary_u256_block(u, ctx, depth + 1)?)),
        U256ExprChoice::Call => {
            let function = ctx.choose_function_returning(ValueType::U256, u)?;
            let args = ctx.args_for_function(function, u)?;
            Ok(U256Expr::Call { function, args })
        }
    }
}

fn arbitrary_bool_expr(
    u: &mut Unstructured<'_>,
    ctx: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<BoolExpr> {
    let mut choices = vec![BoolExprChoice::Const];

    if ctx.has_bool_values() {
        choices.push(BoolExprChoice::Value);
        choices.push(BoolExprChoice::Binary);
    }
    if ctx.has_u256_values() {
        choices.push(BoolExprChoice::IsZero);
        choices.push(BoolExprChoice::Compare);
    }
    if !ctx.callable_functions_returning(ValueType::Bool).is_empty() {
        choices.push(BoolExprChoice::Call);
    }
    if depth < MAX_BLOCK_DEPTH {
        choices.push(BoolExprChoice::Block);
        if ctx.has_bool_values() {
            choices.push(BoolExprChoice::If);
        }
    }

    let choice = choices[u.int_in_range(0..=choices.len() - 1)?];
    match choice {
        BoolExprChoice::Const => Ok(BoolExpr::Const(bool::arbitrary(u)?)),
        BoolExprChoice::Value => Ok(BoolExpr::Value(ctx.choose_bool_value(u)?)),
        BoolExprChoice::IsZero => Ok(BoolExpr::IsZero(ctx.choose_u256_value(u)?)),
        BoolExprChoice::Compare => Ok(BoolExpr::Compare {
            op: arbitrary_compare_op(u)?,
            left: ctx.choose_u256_value(u)?,
            right: ctx.choose_u256_value(u)?,
        }),
        BoolExprChoice::Binary => Ok(BoolExpr::Binary {
            op: arbitrary_bool_binary_op(u)?,
            left: ctx.choose_bool_value(u)?,
            right: ctx.choose_bool_value(u)?,
        }),
        BoolExprChoice::If => Ok(BoolExpr::If {
            cond: ctx.choose_bool_value(u)?,
            then_block: arbitrary_bool_block(u, ctx, depth + 1)?,
            else_block: arbitrary_bool_block(u, ctx, depth + 1)?,
        }),
        BoolExprChoice::Block => Ok(BoolExpr::Block(arbitrary_bool_block(u, ctx, depth + 1)?)),
        BoolExprChoice::Call => {
            let function = ctx.choose_function_returning(ValueType::Bool, u)?;
            let args = ctx.args_for_function(function, u)?;
            Ok(BoolExpr::Call { function, args })
        }
    }
}

fn arbitrary_u256_block(
    u: &mut Unstructured<'_>,
    parent: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<Block<crate::generator::ast::U256Value>> {
    let mut ctx = parent.fork();
    let stmts = arbitrary_stmts(u, &mut ctx, MAX_BLOCK_STATEMENTS, depth)?;

    let mut stmts = stmts;
    if !ctx.has_u256_values() {
        let id = ctx.allocate_u256(false);
        stmts.push(Stmt::LetU256 {
            id,
            mutable: false,
            expr: U256Expr::Const(arbitrary_u256_const(u)?),
        });
    }

    let result = ctx.choose_u256_value(u)?;
    parent.absorb_allocations(&ctx);
    Ok(Block { stmts, result })
}

fn arbitrary_bool_block(
    u: &mut Unstructured<'_>,
    parent: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<Block<crate::generator::ast::BoolValue>> {
    let mut ctx = parent.fork();
    let stmts = arbitrary_stmts(u, &mut ctx, MAX_BLOCK_STATEMENTS, depth)?;

    let mut stmts = stmts;
    if !ctx.has_bool_values() {
        let id = ctx.allocate_bool(false);
        stmts.push(Stmt::LetBool {
            id,
            mutable: false,
            expr: BoolExpr::Const(bool::arbitrary(u)?),
        });
    }

    let result = ctx.choose_bool_value(u)?;
    parent.absorb_allocations(&ctx);
    Ok(Block { stmts, result })
}

fn arbitrary_stmt_block(
    u: &mut Unstructured<'_>,
    parent: &mut GenerationContext,
    depth: u8,
) -> arbitrary::Result<StmtBlock> {
    let mut ctx = parent.fork();
    let stmts = arbitrary_stmts(u, &mut ctx, MAX_BLOCK_STATEMENTS, depth)?;
    parent.absorb_allocations(&ctx);
    Ok(StmtBlock { stmts })
}

fn arbitrary_u256_const(u: &mut Unstructured<'_>) -> arbitrary::Result<U256Const> {
    match u.int_in_range(0..=9)? {
        0 => Ok(U256Const::zero()),
        1 => Ok(U256Const::one()),
        2 => Ok(U256Const::from_u64(255)),
        3 => Ok(U256Const::power_of_two(128)),
        4 => Ok(U256Const::power_of_two(255)),
        5 => Ok(U256Const::max()),
        6 => Ok(U256Const::from_u64(u64::from(u8::arbitrary(u)?))),
        7 => Ok(U256Const::from_u64(u64::from(u16::arbitrary(u)?))),
        8 => Ok(U256Const::from_u64(u64::arbitrary(u)?)),
        9 => {
            let mut bytes = [0; 32];
            for byte in &mut bytes {
                *byte = u8::arbitrary(u)?;
            }
            Ok(U256Const::from_be_bytes(bytes))
        }
        _ => unreachable!("const choice is clamped to valid alternatives"),
    }
}

fn arbitrary_u256_binary_op(u: &mut Unstructured<'_>) -> arbitrary::Result<U256BinaryOp> {
    match u.int_in_range(0..=11)? {
        0 => Ok(U256BinaryOp::Add),
        1 => Ok(U256BinaryOp::Sub),
        2 => Ok(U256BinaryOp::Mul),
        3 => Ok(U256BinaryOp::Div),
        4 => Ok(U256BinaryOp::Mod),
        5 => Ok(U256BinaryOp::And),
        6 => Ok(U256BinaryOp::Or),
        7 => Ok(U256BinaryOp::Xor),
        8 => Ok(U256BinaryOp::Shl),
        9 => Ok(U256BinaryOp::Shr),
        10 => Ok(U256BinaryOp::Sar),
        11 => Ok(U256BinaryOp::Byte),
        _ => unreachable!("binary op choice is clamped to valid alternatives"),
    }
}

fn arbitrary_u256_ternary_op(u: &mut Unstructured<'_>) -> arbitrary::Result<U256TernaryOp> {
    match u.int_in_range(0..=1)? {
        0 => Ok(U256TernaryOp::AddMod),
        1 => Ok(U256TernaryOp::MulMod),
        _ => unreachable!("ternary op choice is clamped to valid alternatives"),
    }
}

fn arbitrary_compare_op(u: &mut Unstructured<'_>) -> arbitrary::Result<CompareOp> {
    match u.int_in_range(0..=4)? {
        0 => Ok(CompareOp::Eq),
        1 => Ok(CompareOp::Lt),
        2 => Ok(CompareOp::Gt),
        3 => Ok(CompareOp::SLt),
        4 => Ok(CompareOp::SGt),
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

#[derive(Debug, Clone, Copy)]
enum StmtChoice {
    LetU256,
    LetBool,
    AssignU256,
    AssignBool,
    If,
}

#[derive(Debug, Clone, Copy)]
enum U256ExprChoice {
    Const,
    Value,
    Unary,
    Binary,
    Ternary,
    If,
    Block,
    Call,
}

#[derive(Debug, Clone, Copy)]
enum BoolExprChoice {
    Const,
    Value,
    IsZero,
    Compare,
    Binary,
    If,
    Block,
    Call,
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
