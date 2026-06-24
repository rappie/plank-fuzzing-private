#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expr {
    Const(u64),
    CalldataWord0,
    CalldataWord1,
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Xor,
    And,
    Or,
}

impl Expr {
    pub(crate) fn binary(op: BinaryOp, left: Expr, right: Expr) -> Self {
        Self::Binary { op, left: Box::new(left), right: Box::new(right) }
    }
}

impl BinaryOp {
    fn plank_token(self) -> &'static str {
        match self {
            Self::Add => "+%",
            Self::Sub => "-%",
            Self::Mul => "*%",
            Self::Xor => "^",
            Self::And => "&",
            Self::Or => "|",
        }
    }
}

pub(crate) fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Const(value) => value.to_string(),
        Expr::CalldataWord0 => "a".to_string(),
        Expr::CalldataWord1 => "b".to_string(),
        Expr::Binary { op, left, right } => {
            format!("({} {} {})", render_expr(left), op.plank_token(), render_expr(right))
        }
    }
}
