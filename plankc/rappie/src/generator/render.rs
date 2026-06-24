use crate::generator::ast::{
    ArgValue, Block, BoolExpr, BoolRef, BoolValue, Function, FunctionBody, FunctionRef, ParamRef,
    Program, Stmt, StmtBlock, U256Expr, U256Ref, U256Value,
};
use std::fmt::Write;

const INDENT: &str = "    ";

pub(crate) fn render_program(program: &Program) -> String {
    debug_assert!(program.validate().is_ok());

    let mut source = String::new();

    for function in &program.functions {
        render_function(&mut source, function);
        source.push('\n');
    }

    source.push_str("init {\n");

    for input in 0..program.input_words {
        let offset = u64::from(input) * 32;
        writeln!(source, "{INDENT}let in{input} = @evm_calldataload({offset});")
            .expect("writing to a string cannot fail");
    }

    if !program.stmts.is_empty() {
        source.push('\n');
    }

    for stmt in &program.stmts {
        render_stmt(&mut source, stmt, 1);
    }

    source.push('\n');
    source.push_str("    let out = @malloc_uninit(32);\n");
    writeln!(source, "    @mstore32(out, {});", render_u256_value(program.result))
        .expect("writing to a string cannot fail");
    source.push_str("    @evm_return(out, 32);\n");
    source.push_str("}\n");

    source
}

fn render_function(source: &mut String, function: &Function) {
    write!(source, "const {} = fn(", render_function_ref(function.id))
        .expect("writing to a string cannot fail");

    for (index, param) in function.params.iter().enumerate() {
        if index > 0 {
            source.push_str(", ");
        }
        write!(source, "{}: {}", render_param_ref(ParamRef::new(index)), param.plank_name())
            .expect("writing to a string cannot fail");
    }

    writeln!(source, ") {} {{", function.return_type.plank_name())
        .expect("writing to a string cannot fail");

    match &function.body {
        FunctionBody::U256(body) => render_u256_block_contents(source, body, 1),
        FunctionBody::Bool(body) => render_bool_block_contents(source, body, 1),
    }

    source.push_str("};\n");
}

fn render_stmt(source: &mut String, stmt: &Stmt, indent: usize) {
    match stmt {
        Stmt::LetU256 { id, mutable, expr } => {
            render_let(
                source,
                indent,
                *mutable,
                &render_u256_ref(*id),
                &render_u256_expr(expr, indent),
            );
        }
        Stmt::LetBool { id, mutable, expr } => {
            render_let(
                source,
                indent,
                *mutable,
                &render_bool_ref(*id),
                &render_bool_expr(expr, indent),
            );
        }
        Stmt::AssignU256 { target, expr } => {
            writeln!(
                source,
                "{}{} = {};",
                indent_str(indent),
                render_u256_ref(*target),
                render_u256_expr(expr, indent)
            )
            .expect("writing to a string cannot fail");
        }
        Stmt::AssignBool { target, expr } => {
            writeln!(
                source,
                "{}{} = {};",
                indent_str(indent),
                render_bool_ref(*target),
                render_bool_expr(expr, indent)
            )
            .expect("writing to a string cannot fail");
        }
        Stmt::If { cond, then_block, else_block } => {
            writeln!(source, "{}if {} {{", indent_str(indent), render_bool_value(*cond))
                .expect("writing to a string cannot fail");
            render_stmt_block_contents(source, then_block, indent + 1);
            writeln!(source, "{}}} else {{", indent_str(indent))
                .expect("writing to a string cannot fail");
            render_stmt_block_contents(source, else_block, indent + 1);
            writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
        }
    }
}

fn render_let(source: &mut String, indent: usize, mutable: bool, name: &str, expr: &str) {
    let mut_kw = if mutable { "mut " } else { "" };
    writeln!(source, "{}let {mut_kw}{name} = {expr};", indent_str(indent))
        .expect("writing to a string cannot fail");
}

fn render_u256_expr(expr: &U256Expr, indent: usize) -> String {
    match expr {
        U256Expr::Const(value) => value.render(),
        U256Expr::Value(value) => render_u256_value(*value),
        U256Expr::Unary { op, value } => {
            format!("{}({})", op.builtin_name(), render_u256_value(*value))
        }
        U256Expr::Binary { op, left, right } => {
            format!(
                "{}({}, {})",
                op.builtin_name(),
                render_u256_value(*left),
                render_u256_value(*right)
            )
        }
        U256Expr::Ternary { op, first, second, third } => {
            format!(
                "{}({}, {}, {})",
                op.builtin_name(),
                render_u256_value(*first),
                render_u256_value(*second),
                render_u256_value(*third)
            )
        }
        U256Expr::If { cond, then_block, else_block } => render_if_expr(
            render_bool_value(*cond),
            |source| render_u256_block_contents(source, then_block, indent + 1),
            |source| render_u256_block_contents(source, else_block, indent + 1),
            indent,
        ),
        U256Expr::Block(block) => render_block_expr(
            |source| render_u256_block_contents(source, block, indent + 1),
            indent,
        ),
        U256Expr::Call { function, args } => render_call(*function, args),
    }
}

fn render_bool_expr(expr: &BoolExpr, indent: usize) -> String {
    match expr {
        BoolExpr::Const(value) => value.to_string(),
        BoolExpr::Value(value) => render_bool_value(*value),
        BoolExpr::IsZero(value) => format!("@evm_iszero({})", render_u256_value(*value)),
        BoolExpr::Compare { op, left, right } => {
            format!(
                "{}({}, {})",
                op.builtin_name(),
                render_u256_value(*left),
                render_u256_value(*right)
            )
        }
        BoolExpr::Binary { op, left, right } => {
            format!(
                "{}({}, {})",
                op.builtin_name(),
                render_bool_value(*left),
                render_bool_value(*right)
            )
        }
        BoolExpr::If { cond, then_block, else_block } => render_if_expr(
            render_bool_value(*cond),
            |source| render_bool_block_contents(source, then_block, indent + 1),
            |source| render_bool_block_contents(source, else_block, indent + 1),
            indent,
        ),
        BoolExpr::Block(block) => render_block_expr(
            |source| render_bool_block_contents(source, block, indent + 1),
            indent,
        ),
        BoolExpr::Call { function, args } => render_call(*function, args),
    }
}

fn render_if_expr(
    cond: String,
    then_body: impl FnOnce(&mut String),
    else_body: impl FnOnce(&mut String),
    indent: usize,
) -> String {
    let mut rendered = String::new();
    writeln!(rendered, "if {cond} {{").expect("writing to a string cannot fail");
    then_body(&mut rendered);
    writeln!(rendered, "{}}} else {{", indent_str(indent))
        .expect("writing to a string cannot fail");
    else_body(&mut rendered);
    write!(rendered, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
    rendered
}

fn render_block_expr(body: impl FnOnce(&mut String), indent: usize) -> String {
    let mut rendered = String::new();
    rendered.push_str("{\n");
    body(&mut rendered);
    write!(rendered, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
    rendered
}

fn render_stmt_block_contents(source: &mut String, block: &StmtBlock, indent: usize) {
    for stmt in &block.stmts {
        render_stmt(source, stmt, indent);
    }
}

fn render_u256_block_contents(source: &mut String, block: &Block<U256Value>, indent: usize) {
    for stmt in &block.stmts {
        render_stmt(source, stmt, indent);
    }
    writeln!(source, "{}{}", indent_str(indent), render_u256_value(block.result))
        .expect("writing to a string cannot fail");
}

fn render_bool_block_contents(source: &mut String, block: &Block<BoolValue>, indent: usize) {
    for stmt in &block.stmts {
        render_stmt(source, stmt, indent);
    }
    writeln!(source, "{}{}", indent_str(indent), render_bool_value(block.result))
        .expect("writing to a string cannot fail");
}

fn render_call(function: FunctionRef, args: &[ArgValue]) -> String {
    let mut rendered = String::new();
    write!(rendered, "{}(", render_function_ref(function))
        .expect("writing to a string cannot fail");

    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&render_arg_value(arg));
    }

    rendered.push(')');
    rendered
}

fn render_arg_value(arg: &ArgValue) -> String {
    match arg {
        ArgValue::U256(value) => render_u256_value(*value),
        ArgValue::Bool(value) => render_bool_value(*value),
    }
}

fn render_u256_value(value: U256Value) -> String {
    match value {
        U256Value::Input(index) => format!("in{index}"),
        U256Value::Param(param) => render_param_ref(param),
        U256Value::Local(local) => render_u256_ref(local),
    }
}

fn render_bool_value(value: BoolValue) -> String {
    match value {
        BoolValue::Param(param) => render_param_ref(param),
        BoolValue::Local(local) => render_bool_ref(local),
    }
}

fn render_u256_ref(local: U256Ref) -> String {
    format!("v{}", local.index())
}

fn render_bool_ref(local: BoolRef) -> String {
    format!("b{}", local.index())
}

fn render_param_ref(param: ParamRef) -> String {
    format!("x{}", param.index())
}

fn render_function_ref(function: FunctionRef) -> String {
    format!("f{}", function.index())
}

fn indent_str(indent: usize) -> String {
    INDENT.repeat(indent)
}

#[cfg(test)]
mod tests {
    use super::render_program;
    use crate::generator::ast::{
        Block, BoolExpr, BoolRef, BoolValue, CompareOp, Function, FunctionBody, FunctionRef,
        Program, Stmt, U256BinaryOp, U256Const, U256Expr, U256Ref, U256Value, ValueType,
    };

    #[test]
    fn renders_hand_constructed_program() {
        let program = Program::new(
            2,
            vec![
                Stmt::LetU256 {
                    id: U256Ref::new(0),
                    mutable: false,
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
                        op: CompareOp::Lt,
                        left: U256Value::Local(U256Ref::new(0)),
                        right: U256Value::Input(0),
                    },
                },
                Stmt::LetU256 {
                    id: U256Ref::new(1),
                    mutable: false,
                    expr: U256Expr::If {
                        cond: BoolValue::Local(BoolRef::new(0)),
                        then_block: Block {
                            stmts: vec![Stmt::LetU256 {
                                id: U256Ref::new(2),
                                mutable: false,
                                expr: U256Expr::Const(U256Const::one()),
                            }],
                            result: U256Value::Local(U256Ref::new(2)),
                        },
                        else_block: Block { stmts: Vec::new(), result: U256Value::Input(1) },
                    },
                },
            ],
            U256Value::Local(U256Ref::new(1)),
        );

        assert_eq!(
            render_program(&program),
            r#"init {
    let in0 = @evm_calldataload(0);
    let in1 = @evm_calldataload(32);

    let v0 = @evm_add(in0, in1);
    let b0 = @evm_lt(v0, in0);
    let v1 = if b0 {
        let v2 = 0x1;
        v2
    } else {
        in1
    };

    let out = @malloc_uninit(32);
    @mstore32(out, v1);
    @evm_return(out, 32);
}
"#
        );
    }

    #[test]
    fn renders_functions_before_init() {
        let program = Program::with_functions(
            1,
            vec![Function {
                id: FunctionRef::new(0),
                params: vec![ValueType::U256],
                return_type: ValueType::U256,
                body: FunctionBody::U256(Block {
                    stmts: Vec::new(),
                    result: U256Value::Param(crate::generator::ast::ParamRef::new(0)),
                }),
            }],
            vec![Stmt::LetU256 {
                id: U256Ref::new(0),
                mutable: false,
                expr: U256Expr::Call {
                    function: FunctionRef::new(0),
                    args: vec![crate::generator::ast::ArgValue::U256(U256Value::Input(0))],
                },
            }],
            U256Value::Local(U256Ref::new(0)),
        );

        assert!(render_program(&program).starts_with("const f0 = fn(x0: u256) u256 {\n"));
    }
}
