use crate::{
    expr::{BinaryOp, Expr},
    program::render_program,
};
use alloy_primitives::U256;
use arbitrary::{Arbitrary, Unstructured};

const MAX_ARBITRARY_EXPR_DEPTH: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzCase {
    expr: Expr,
    calldata_a: u64,
    calldata_b: u64,
}

impl FuzzCase {
    pub fn source(&self) -> String {
        render_program(&self.expr)
    }

    pub fn calldata(&self) -> Vec<u8> {
        calldata_words([self.calldata_a, self.calldata_b])
    }
}

impl<'a> Arbitrary<'a> for FuzzCase {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            expr: arbitrary_expr(u, MAX_ARBITRARY_EXPR_DEPTH)?,
            calldata_a: u64::arbitrary(u)?,
            calldata_b: u64::arbitrary(u)?,
        })
    }
}

fn arbitrary_expr(u: &mut Unstructured<'_>, depth: u8) -> arbitrary::Result<Expr> {
    if depth == 0 {
        return arbitrary_leaf(u);
    }

    match u.int_in_range(0..=8)? {
        0..=2 => arbitrary_leaf(u),
        3 => arbitrary_binary_expr(u, depth, BinaryOp::Add),
        4 => arbitrary_binary_expr(u, depth, BinaryOp::Sub),
        5 => arbitrary_binary_expr(u, depth, BinaryOp::Mul),
        6 => arbitrary_binary_expr(u, depth, BinaryOp::Xor),
        7 => arbitrary_binary_expr(u, depth, BinaryOp::And),
        8 => arbitrary_binary_expr(u, depth, BinaryOp::Or),
        _ => unreachable!("int_in_range(0..=8) returns 0..=8"),
    }
}

fn arbitrary_leaf(u: &mut Unstructured<'_>) -> arbitrary::Result<Expr> {
    match u.int_in_range(0..=2)? {
        0 => Ok(Expr::Const(u64::from(u16::arbitrary(u)?))),
        1 => Ok(Expr::CalldataWord0),
        2 => Ok(Expr::CalldataWord1),
        _ => unreachable!("int_in_range(0..=2) returns 0..=2"),
    }
}

fn arbitrary_binary_expr(
    u: &mut Unstructured<'_>,
    depth: u8,
    op: BinaryOp,
) -> arbitrary::Result<Expr> {
    let next_depth = depth - 1;
    Ok(Expr::binary(op, arbitrary_expr(u, next_depth)?, arbitrary_expr(u, next_depth)?))
}

fn calldata_words(words: impl IntoIterator<Item = u64>) -> Vec<u8> {
    let mut calldata = Vec::new();
    for word in words {
        calldata.extend_from_slice(&U256::from(word).to_be_bytes::<32>());
    }
    calldata
}
