use std::fmt;

pub(crate) const MIN_INPUT_WORDS: u8 = 1;
pub(crate) const MAX_INPUT_WORDS: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Program {
    pub(crate) input_words: u8,
    pub(crate) functions: Vec<Function>,
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) result: U256Value,
}

impl Program {
    #[cfg(test)]
    pub(crate) fn new(input_words: u8, stmts: Vec<Stmt>, result: U256Value) -> Self {
        Self { input_words, functions: Vec::new(), stmts, result }
    }

    pub(crate) fn with_functions(
        input_words: u8,
        functions: Vec<Function>,
        stmts: Vec<Stmt>,
        result: U256Value,
    ) -> Self {
        Self { input_words, functions, stmts, result }
    }

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        if !(MIN_INPUT_WORDS..=MAX_INPUT_WORDS).contains(&self.input_words) {
            return Err(ValidationError::new(format!(
                "input word count {} outside {MIN_INPUT_WORDS}..={MAX_INPUT_WORDS}",
                self.input_words
            )));
        }

        let mut visible_functions = Vec::new();
        for (index, function) in self.functions.iter().enumerate() {
            if function.id.index() != index {
                return Err(ValidationError::new(format!(
                    "function {} was declared out of order",
                    function.id.index()
                )));
            }

            validate_function(function, &visible_functions)?;
            visible_functions.push(function.signature());
        }

        let mut scope = ValidationScope::for_init(self.input_words, visible_functions);
        validate_stmts(&self.stmts, &mut scope)?;
        validate_u256_value(self.result, &scope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Function {
    pub(crate) id: FunctionRef,
    pub(crate) params: Vec<ValueType>,
    pub(crate) return_type: ValueType,
    pub(crate) body: FunctionBody,
}

impl Function {
    pub(crate) fn signature(&self) -> FunctionSignature {
        FunctionSignature {
            id: self.id,
            params: self.params.clone(),
            return_type: self.return_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FunctionBody {
    U256(Block<U256Value>),
    Bool(Block<BoolValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FunctionSignature {
    pub(crate) id: FunctionRef,
    pub(crate) params: Vec<ValueType>,
    pub(crate) return_type: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block<T> {
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) result: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StmtBlock {
    pub(crate) stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stmt {
    LetU256 { id: U256Ref, mutable: bool, expr: U256Expr },
    LetBool { id: BoolRef, mutable: bool, expr: BoolExpr },
    AssignU256 { target: U256Ref, expr: U256Expr },
    AssignBool { target: BoolRef, expr: BoolExpr },
    If { cond: BoolValue, then_block: StmtBlock, else_block: StmtBlock },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum U256Expr {
    Const(U256Const),
    Value(U256Value),
    Unary { op: U256UnaryOp, value: U256Value },
    Binary { op: U256BinaryOp, left: U256Value, right: U256Value },
    Ternary { op: U256TernaryOp, first: U256Value, second: U256Value, third: U256Value },
    If { cond: BoolValue, then_block: Block<U256Value>, else_block: Block<U256Value> },
    Block(Block<U256Value>),
    Call { function: FunctionRef, args: Vec<ArgValue> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoolExpr {
    Const(bool),
    Value(BoolValue),
    IsZero(U256Value),
    Compare { op: CompareOp, left: U256Value, right: U256Value },
    Binary { op: BoolBinaryOp, left: BoolValue, right: BoolValue },
    If { cond: BoolValue, then_block: Block<BoolValue>, else_block: Block<BoolValue> },
    Block(Block<BoolValue>),
    Call { function: FunctionRef, args: Vec<ArgValue> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256Value {
    Input(u8),
    Param(ParamRef),
    Local(U256Ref),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoolValue {
    Param(ParamRef),
    Local(BoolRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArgValue {
    U256(U256Value),
    Bool(BoolValue),
}

impl ArgValue {
    fn value_type(&self) -> ValueType {
        match self {
            Self::U256(_) => ValueType::U256,
            Self::Bool(_) => ValueType::Bool,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueType {
    U256,
    Bool,
}

impl ValueType {
    pub(crate) fn plank_name(self) -> &'static str {
        match self {
            Self::U256 => "u256",
            Self::Bool => "bool",
        }
    }
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
pub(crate) struct ParamRef(usize);

impl ParamRef {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FunctionRef(usize);

impl FunctionRef {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct U256Const([u8; 32]);

impl U256Const {
    pub(crate) fn zero() -> Self {
        Self([0; 32])
    }

    pub(crate) fn one() -> Self {
        Self::from_u64(1)
    }

    pub(crate) fn from_u64(value: u64) -> Self {
        let mut bytes = [0; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        Self(bytes)
    }

    pub(crate) fn from_be_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn power_of_two(bit: u16) -> Self {
        debug_assert!(bit < 256);

        let mut bytes = [0; 32];
        let byte_index = 31 - usize::from(bit / 8);
        bytes[byte_index] = 1 << (bit % 8);
        Self(bytes)
    }

    pub(crate) fn max() -> Self {
        Self([0xff; 32])
    }

    pub(crate) fn render(self) -> String {
        let Some(first_non_zero) = self.0.iter().position(|&byte| byte != 0) else {
            return "0".to_string();
        };

        let bytes = &self.0[first_non_zero..];
        let mut rendered = String::with_capacity(2 + bytes.len() * 2);
        rendered.push_str("0x");

        let first = bytes[0];
        if first < 0x10 {
            rendered.push(hex_digit(first));
        } else {
            rendered.push(hex_digit(first >> 4));
            rendered.push(hex_digit(first & 0x0f));
        }

        for &byte in &bytes[1..] {
            rendered.push(hex_digit(byte >> 4));
            rendered.push(hex_digit(byte & 0x0f));
        }

        rendered
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
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Sar,
    Byte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum U256TernaryOp {
    AddMod,
    MulMod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Eq,
    Lt,
    Gt,
    SLt,
    SGt,
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
            Self::Div => "@evm_div",
            Self::Mod => "@evm_mod",
            Self::And => "@evm_and",
            Self::Or => "@evm_or",
            Self::Xor => "@evm_xor",
            Self::Shl => "@evm_shl",
            Self::Shr => "@evm_shr",
            Self::Sar => "@evm_sar",
            Self::Byte => "@evm_byte",
        }
    }
}

impl U256TernaryOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::AddMod => "@evm_addmod",
            Self::MulMod => "@evm_mulmod",
        }
    }
}

impl CompareOp {
    pub(crate) fn builtin_name(self) -> &'static str {
        match self {
            Self::Eq => "@evm_eq",
            Self::Lt => "@evm_lt",
            Self::Gt => "@evm_gt",
            Self::SLt => "@evm_slt",
            Self::SGt => "@evm_sgt",
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

#[derive(Debug, Clone)]
struct ValidationScope {
    input_words: u8,
    params: Vec<ValueType>,
    u256_locals: Vec<LocalState<U256Ref>>,
    bool_locals: Vec<LocalState<BoolRef>>,
    functions: Vec<FunctionSignature>,
}

#[derive(Debug, Clone, Copy)]
struct LocalState<T> {
    id: T,
    mutable: bool,
}

impl ValidationScope {
    fn for_init(input_words: u8, functions: Vec<FunctionSignature>) -> Self {
        Self {
            input_words,
            params: Vec::new(),
            u256_locals: Vec::new(),
            bool_locals: Vec::new(),
            functions,
        }
    }

    fn for_function(params: Vec<ValueType>, functions: Vec<FunctionSignature>) -> Self {
        Self { input_words: 0, params, u256_locals: Vec::new(), bool_locals: Vec::new(), functions }
    }

    fn add_u256_local(&mut self, id: U256Ref, mutable: bool) -> Result<(), ValidationError> {
        if self.u256_locals.iter().any(|local| local.id == id) {
            return Err(ValidationError::new(format!(
                "u256 local {} was declared twice in the same scope",
                id.index()
            )));
        }

        self.u256_locals.push(LocalState { id, mutable });
        Ok(())
    }

    fn add_bool_local(&mut self, id: BoolRef, mutable: bool) -> Result<(), ValidationError> {
        if self.bool_locals.iter().any(|local| local.id == id) {
            return Err(ValidationError::new(format!(
                "bool local {} was declared twice in the same scope",
                id.index()
            )));
        }

        self.bool_locals.push(LocalState { id, mutable });
        Ok(())
    }

    fn u256_local(&self, id: U256Ref) -> Option<LocalState<U256Ref>> {
        self.u256_locals.iter().copied().find(|local| local.id == id)
    }

    fn bool_local(&self, id: BoolRef) -> Option<LocalState<BoolRef>> {
        self.bool_locals.iter().copied().find(|local| local.id == id)
    }

    fn function(&self, id: FunctionRef) -> Option<&FunctionSignature> {
        self.functions.iter().find(|function| function.id == id)
    }
}

fn validate_function(
    function: &Function,
    visible_functions: &[FunctionSignature],
) -> Result<(), ValidationError> {
    let scope = ValidationScope::for_function(function.params.clone(), visible_functions.to_vec());

    match (&function.return_type, &function.body) {
        (ValueType::U256, FunctionBody::U256(body)) => validate_u256_block(body, &scope),
        (ValueType::Bool, FunctionBody::Bool(body)) => validate_bool_block(body, &scope),
        _ => Err(ValidationError::new(format!(
            "function {} body type does not match declared return type",
            function.id.index()
        ))),
    }
}

fn validate_stmts(stmts: &[Stmt], scope: &mut ValidationScope) -> Result<(), ValidationError> {
    for stmt in stmts {
        validate_stmt(stmt, scope)?;
    }

    Ok(())
}

fn validate_stmt(stmt: &Stmt, scope: &mut ValidationScope) -> Result<(), ValidationError> {
    match stmt {
        Stmt::LetU256 { id, mutable, expr } => {
            validate_u256_expr(expr, scope)?;
            scope.add_u256_local(*id, *mutable)
        }
        Stmt::LetBool { id, mutable, expr } => {
            validate_bool_expr(expr, scope)?;
            scope.add_bool_local(*id, *mutable)
        }
        Stmt::AssignU256 { target, expr } => {
            let Some(local) = scope.u256_local(*target) else {
                return Err(ValidationError::new(format!(
                    "assignment target u256 local {} is not visible",
                    target.index()
                )));
            };
            if !local.mutable {
                return Err(ValidationError::new(format!(
                    "assignment target u256 local {} is not mutable",
                    target.index()
                )));
            }
            validate_u256_expr(expr, scope)
        }
        Stmt::AssignBool { target, expr } => {
            let Some(local) = scope.bool_local(*target) else {
                return Err(ValidationError::new(format!(
                    "assignment target bool local {} is not visible",
                    target.index()
                )));
            };
            if !local.mutable {
                return Err(ValidationError::new(format!(
                    "assignment target bool local {} is not mutable",
                    target.index()
                )));
            }
            validate_bool_expr(expr, scope)
        }
        Stmt::If { cond, then_block, else_block } => {
            validate_bool_value(*cond, scope)?;
            validate_stmt_block(then_block, scope)?;
            validate_stmt_block(else_block, scope)
        }
    }
}

fn validate_stmt_block(block: &StmtBlock, parent: &ValidationScope) -> Result<(), ValidationError> {
    let mut scope = parent.clone();
    validate_stmts(&block.stmts, &mut scope)
}

fn validate_u256_block(
    block: &Block<U256Value>,
    parent: &ValidationScope,
) -> Result<(), ValidationError> {
    let mut scope = parent.clone();
    validate_stmts(&block.stmts, &mut scope)?;
    validate_u256_value(block.result, &scope)
}

fn validate_bool_block(
    block: &Block<BoolValue>,
    parent: &ValidationScope,
) -> Result<(), ValidationError> {
    let mut scope = parent.clone();
    validate_stmts(&block.stmts, &mut scope)?;
    validate_bool_value(block.result, &scope)
}

fn validate_u256_expr(expr: &U256Expr, scope: &ValidationScope) -> Result<(), ValidationError> {
    match expr {
        U256Expr::Const(_) => Ok(()),
        U256Expr::Value(value) | U256Expr::Unary { value, .. } => {
            validate_u256_value(*value, scope)
        }
        U256Expr::Binary { left, right, .. } => {
            validate_u256_value(*left, scope)?;
            validate_u256_value(*right, scope)
        }
        U256Expr::Ternary { first, second, third, .. } => {
            validate_u256_value(*first, scope)?;
            validate_u256_value(*second, scope)?;
            validate_u256_value(*third, scope)
        }
        U256Expr::If { cond, then_block, else_block } => {
            validate_bool_value(*cond, scope)?;
            validate_u256_block(then_block, scope)?;
            validate_u256_block(else_block, scope)
        }
        U256Expr::Block(block) => validate_u256_block(block, scope),
        U256Expr::Call { function, args } => validate_call(*function, args, ValueType::U256, scope),
    }
}

fn validate_bool_expr(expr: &BoolExpr, scope: &ValidationScope) -> Result<(), ValidationError> {
    match expr {
        BoolExpr::Const(_) => Ok(()),
        BoolExpr::Value(value) => validate_bool_value(*value, scope),
        BoolExpr::IsZero(value) => validate_u256_value(*value, scope),
        BoolExpr::Compare { left, right, .. } => {
            validate_u256_value(*left, scope)?;
            validate_u256_value(*right, scope)
        }
        BoolExpr::Binary { left, right, .. } => {
            validate_bool_value(*left, scope)?;
            validate_bool_value(*right, scope)
        }
        BoolExpr::If { cond, then_block, else_block } => {
            validate_bool_value(*cond, scope)?;
            validate_bool_block(then_block, scope)?;
            validate_bool_block(else_block, scope)
        }
        BoolExpr::Block(block) => validate_bool_block(block, scope),
        BoolExpr::Call { function, args } => validate_call(*function, args, ValueType::Bool, scope),
    }
}

fn validate_call(
    function: FunctionRef,
    args: &[ArgValue],
    expected_return_type: ValueType,
    scope: &ValidationScope,
) -> Result<(), ValidationError> {
    let Some(signature) = scope.function(function) else {
        return Err(ValidationError::new(format!("function {} is not visible", function.index())));
    };

    if signature.return_type != expected_return_type {
        return Err(ValidationError::new(format!(
            "function {} return type does not match call context",
            function.index()
        )));
    }

    if signature.params.len() != args.len() {
        return Err(ValidationError::new(format!(
            "function {} called with {} arguments, expected {}",
            function.index(),
            args.len(),
            signature.params.len()
        )));
    }

    for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
        if arg.value_type() != *expected {
            return Err(ValidationError::new(format!(
                "function {} argument {index} has wrong type",
                function.index()
            )));
        }

        match arg {
            ArgValue::U256(value) => validate_u256_value(*value, scope)?,
            ArgValue::Bool(value) => validate_bool_value(*value, scope)?,
        }
    }

    Ok(())
}

fn validate_u256_value(value: U256Value, scope: &ValidationScope) -> Result<(), ValidationError> {
    match value {
        U256Value::Input(index) if index < scope.input_words => Ok(()),
        U256Value::Input(index) => Err(ValidationError::new(format!(
            "input reference {index} outside generated input range 0..{}",
            scope.input_words
        ))),
        U256Value::Param(param) => validate_param(param, ValueType::U256, scope),
        U256Value::Local(local) if scope.u256_local(local).is_some() => Ok(()),
        U256Value::Local(local) => Err(ValidationError::new(format!(
            "u256 local reference {} is not visible",
            local.index()
        ))),
    }
}

fn validate_bool_value(value: BoolValue, scope: &ValidationScope) -> Result<(), ValidationError> {
    match value {
        BoolValue::Param(param) => validate_param(param, ValueType::Bool, scope),
        BoolValue::Local(local) if scope.bool_local(local).is_some() => Ok(()),
        BoolValue::Local(local) => Err(ValidationError::new(format!(
            "bool local reference {} is not visible",
            local.index()
        ))),
    }
}

fn validate_param(
    param: ParamRef,
    expected_type: ValueType,
    scope: &ValidationScope,
) -> Result<(), ValidationError> {
    match scope.params.get(param.index()) {
        Some(actual_type) if *actual_type == expected_type => Ok(()),
        Some(_) => Err(ValidationError::new(format!("parameter {} has wrong type", param.index()))),
        None => Err(ValidationError::new(format!("parameter {} is not visible", param.index()))),
    }
}

fn hex_digit(nibble: u8) -> char {
    debug_assert!(nibble < 16);

    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => unreachable!("nibble must be less than 16"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Block, BoolExpr, BoolRef, Function, FunctionBody, FunctionRef, Program, Stmt, U256BinaryOp,
        U256Const, U256Expr, U256Ref, U256Value, ValidationError, ValueType,
    };

    #[test]
    fn validation_accepts_backward_references() -> Result<(), ValidationError> {
        let program = Program::new(
            1,
            vec![Stmt::LetU256 {
                id: U256Ref::new(0),
                mutable: false,
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
    fn validation_rejects_refs_that_leak_from_blocks() {
        let program = Program::new(
            1,
            vec![Stmt::LetU256 {
                id: U256Ref::new(0),
                mutable: false,
                expr: U256Expr::Block(Block {
                    stmts: vec![Stmt::LetU256 {
                        id: U256Ref::new(1),
                        mutable: false,
                        expr: U256Expr::Const(U256Const::one()),
                    }],
                    result: U256Value::Local(U256Ref::new(1)),
                }),
            }],
            U256Value::Local(U256Ref::new(1)),
        );

        let err = program.validate().expect_err("block local should not leak into parent");
        assert!(err.to_string().contains("not visible"));
    }

    #[test]
    fn validation_rejects_duplicate_locals_in_the_same_scope() {
        let program = Program::new(
            1,
            vec![
                Stmt::LetBool { id: BoolRef::new(0), mutable: false, expr: BoolExpr::Const(true) },
                Stmt::LetBool { id: BoolRef::new(0), mutable: false, expr: BoolExpr::Const(false) },
            ],
            U256Value::Input(0),
        );

        let err = program.validate().expect_err("duplicate local should be rejected");
        assert!(err.to_string().contains("declared twice"));
    }

    #[test]
    fn validation_rejects_assignment_to_immutable_local() {
        let program = Program::new(
            1,
            vec![
                Stmt::LetU256 {
                    id: U256Ref::new(0),
                    mutable: false,
                    expr: U256Expr::Value(U256Value::Input(0)),
                },
                Stmt::AssignU256 {
                    target: U256Ref::new(0),
                    expr: U256Expr::Const(U256Const::one()),
                },
            ],
            U256Value::Local(U256Ref::new(0)),
        );

        let err = program.validate().expect_err("immutable assignment should be rejected");
        assert!(err.to_string().contains("not mutable"));
    }

    #[test]
    fn validation_rejects_forward_function_calls() {
        let program = Program::with_functions(
            1,
            vec![
                Function {
                    id: FunctionRef::new(0),
                    params: Vec::new(),
                    return_type: ValueType::U256,
                    body: FunctionBody::U256(Block {
                        stmts: vec![Stmt::LetU256 {
                            id: U256Ref::new(0),
                            mutable: false,
                            expr: U256Expr::Const(U256Const::one()),
                        }],
                        result: U256Value::Local(U256Ref::new(0)),
                    }),
                },
                Function {
                    id: FunctionRef::new(1),
                    params: Vec::new(),
                    return_type: ValueType::U256,
                    body: FunctionBody::U256(Block {
                        stmts: vec![Stmt::LetU256 {
                            id: U256Ref::new(0),
                            mutable: false,
                            expr: U256Expr::Call { function: FunctionRef::new(1), args: vec![] },
                        }],
                        result: U256Value::Local(U256Ref::new(0)),
                    }),
                },
            ],
            Vec::new(),
            U256Value::Input(0),
        );

        let err = program.validate().expect_err("self call should be rejected");
        assert!(err.to_string().contains("not visible"));
    }

    #[test]
    fn u256_constants_render_as_plank_literals() {
        assert_eq!(U256Const::zero().render(), "0");
        assert_eq!(U256Const::one().render(), "0x1");
        assert_eq!(U256Const::from_u64(255).render(), "0xff");
        assert_eq!(U256Const::max().render(), format!("0x{}", "ff".repeat(32)));
    }
}
