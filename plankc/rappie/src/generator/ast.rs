use std::fmt;

pub(crate) const MIN_INPUT_WORDS: u8 = 1;
pub(crate) const MAX_INPUT_WORDS: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Program {
    pub(crate) input_words: u8,
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) result: U256Value,
}

impl Program {
    pub(crate) fn new(input_words: u8, stmts: Vec<Stmt>, result: U256Value) -> Self {
        Self { input_words, stmts, result }
    }

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        if !(MIN_INPUT_WORDS..=MAX_INPUT_WORDS).contains(&self.input_words) {
            return Err(ValidationError::new(format!(
                "input word count {} outside {MIN_INPUT_WORDS}..={MAX_INPUT_WORDS}",
                self.input_words
            )));
        }

        let mut u256_count = 0;
        let mut bool_count = 0;

        for stmt in &self.stmts {
            match stmt {
                Stmt::LetU256 { id, expr } => {
                    if id.index() != u256_count {
                        return Err(ValidationError::new(format!(
                            "u256 local {} was declared out of order",
                            id.index()
                        )));
                    }
                    validate_u256_expr(expr, self.input_words, u256_count, bool_count)?;
                    u256_count += 1;
                }
                Stmt::LetBool { id, expr } => {
                    if id.index() != bool_count {
                        return Err(ValidationError::new(format!(
                            "bool local {} was declared out of order",
                            id.index()
                        )));
                    }
                    validate_bool_expr(expr, self.input_words, u256_count, bool_count)?;
                    bool_count += 1;
                }
            }
        }

        validate_u256_value(self.result, self.input_words, u256_count)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stmt {
    LetU256 { id: U256Ref, expr: U256Expr },
    LetBool { id: BoolRef, expr: BoolExpr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256Expr {
    Const(u64),
    Value(U256Value),
    Unary { op: U256UnaryOp, value: U256Value },
    Binary { op: U256BinaryOp, left: U256Value, right: U256Value },
    If { cond: BoolValue, then_value: U256Value, else_value: U256Value },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoolExpr {
    Const(bool),
    Value(BoolValue),
    IsZero(U256Value),
    Compare { op: CompareOp, left: U256Value, right: U256Value },
    Binary { op: BoolBinaryOp, left: BoolValue, right: BoolValue },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256Value {
    Input(u8),
    Local(U256Ref),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoolValue {
    Local(BoolRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct U256Ref(usize);

impl U256Ref {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoolRef(usize);

impl BoolRef {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256UnaryOp {
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256BinaryOp {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Eq,
    Lt,
    Gt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoolBinaryOp {
    And,
    Or,
    Xor,
    Eq,
}

impl U256UnaryOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::Not => "@evm_not",
        }
    }
}

impl U256BinaryOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::Add => "@evm_add",
            Self::Sub => "@evm_sub",
            Self::Mul => "@evm_mul",
            Self::And => "@evm_and",
            Self::Or => "@evm_or",
            Self::Xor => "@evm_xor",
        }
    }
}

impl CompareOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::Eq => "@evm_eq",
            Self::Lt => "@evm_lt",
            Self::Gt => "@evm_gt",
        }
    }
}

impl BoolBinaryOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::And => "@evm_and",
            Self::Or => "@evm_or",
            Self::Xor => "@evm_xor",
            Self::Eq => "@evm_eq",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationError {
    message: String,
}

impl ValidationError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValidationError {}

fn validate_u256_expr(
    expr: &U256Expr,
    input_words: u8,
    u256_count: usize,
    bool_count: usize,
) -> Result<(), ValidationError> {
    match expr {
        U256Expr::Const(_) => Ok(()),
        U256Expr::Value(value) | U256Expr::Unary { value, .. } => {
            validate_u256_value(*value, input_words, u256_count)
        }
        U256Expr::Binary { left, right, .. } => {
            validate_u256_value(*left, input_words, u256_count)?;
            validate_u256_value(*right, input_words, u256_count)
        }
        U256Expr::If { cond, then_value, else_value } => {
            validate_bool_value(*cond, bool_count)?;
            validate_u256_value(*then_value, input_words, u256_count)?;
            validate_u256_value(*else_value, input_words, u256_count)
        }
    }
}

fn validate_bool_expr(
    expr: &BoolExpr,
    input_words: u8,
    u256_count: usize,
    bool_count: usize,
) -> Result<(), ValidationError> {
    match expr {
        BoolExpr::Const(_) => Ok(()),
        BoolExpr::Value(value) => validate_bool_value(*value, bool_count),
        BoolExpr::IsZero(value) => validate_u256_value(*value, input_words, u256_count),
        BoolExpr::Compare { left, right, .. } => {
            validate_u256_value(*left, input_words, u256_count)?;
            validate_u256_value(*right, input_words, u256_count)
        }
        BoolExpr::Binary { left, right, .. } => {
            validate_bool_value(*left, bool_count)?;
            validate_bool_value(*right, bool_count)
        }
    }
}

fn validate_u256_value(
    value: U256Value,
    input_words: u8,
    u256_count: usize,
) -> Result<(), ValidationError> {
    match value {
        U256Value::Input(index) if index < input_words => Ok(()),
        U256Value::Input(index) => Err(ValidationError::new(format!(
            "input reference {index} outside generated input range 0..{input_words}"
        ))),
        U256Value::Local(local) if local.index() < u256_count => Ok(()),
        U256Value::Local(local) => Err(ValidationError::new(format!(
            "u256 local reference {} points forward",
            local.index()
        ))),
    }
}

fn validate_bool_value(value: BoolValue, bool_count: usize) -> Result<(), ValidationError> {
    match value {
        BoolValue::Local(local) if local.index() < bool_count => Ok(()),
        BoolValue::Local(local) => Err(ValidationError::new(format!(
            "bool local reference {} points forward",
            local.index()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoolExpr, Program, Stmt, U256BinaryOp, U256Expr, U256Ref, U256Value, ValidationError,
    };

    #[test]
    fn validation_accepts_backward_references() -> Result<(), ValidationError> {
        let program = Program::new(
            1,
            vec![Stmt::LetU256 {
                id: U256Ref::new(0),
                expr: U256Expr::Binary {
                    op: U256BinaryOp::Add,
                    left: U256Value::Input(0),
                    right: U256Value::Input(0),
                },
            }],
            U256Value::Local(U256Ref::new(0)),
        );

        program.validate()
    }

    #[test]
    fn validation_rejects_forward_references() {
        let program = Program::new(
            1,
            vec![Stmt::LetU256 {
                id: U256Ref::new(0),
                expr: U256Expr::Value(U256Value::Local(U256Ref::new(1))),
            }],
            U256Value::Input(0),
        );

        let err = program.validate().expect_err("forward reference should be rejected");
        assert!(err.to_string().contains("points forward"));
    }

    #[test]
    fn validation_rejects_out_of_order_locals() {
        let program = Program::new(
            1,
            vec![Stmt::LetBool { id: super::BoolRef::new(1), expr: BoolExpr::Const(true) }],
            U256Value::Input(0),
        );

        let err = program.validate().expect_err("out-of-order local should be rejected");
        assert!(err.to_string().contains("out of order"));
    }
}
