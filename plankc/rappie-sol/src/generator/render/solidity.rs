use crate::generator::ast::{
    ArgValue, Block, BoolBinaryOp, BoolExpr, BoolRef, BoolValue, CompareOp, Function, FunctionBody,
    FunctionRef, ParamRef, Program, Stmt, StmtBlock, U256BinaryOp, U256Expr, U256Ref,
    U256TernaryOp, U256UnaryOp, U256Value,
};
use std::fmt::Write;

const INDENT: &str = "    ";

pub(crate) fn render_program(program: &Program) -> String {
    debug_assert!(program.validate().is_ok());

    let mut source = String::new();
    source.push_str("// SPDX-License-Identifier: MIT\n");
    source.push_str("pragma solidity >=0.8.20;\n\n");
    source.push_str("contract C {\n");
    source.push_str("    fallback() external payable {\n");
    source.push_str("        assembly (\"memory-safe\") {\n");

    for function in &program.functions {
        render_function(&mut source, function, 3);
        source.push('\n');
    }

    for input in 0..program.input_words {
        let offset = u64::from(input) * 32;
        writeln!(source, "{}let in{input} := calldataload({offset})", indent_str(3))
            .expect("writing to a string cannot fail");
    }

    if !program.stmts.is_empty() {
        source.push('\n');
    }

    for stmt in &program.stmts {
        render_stmt(&mut source, stmt, 3);
    }

    writeln!(source, "{}mstore(0, {})", indent_str(3), render_u256_value(program.result))
        .expect("writing to a string cannot fail");
    source.push_str("            return(0, 32)\n");
    source.push_str("        }\n");
    source.push_str("    }\n");
    source.push_str("}\n");

    source
}

fn render_function(source: &mut String, function: &Function, indent: usize) {
    write!(source, "{}function {}(", indent_str(indent), render_function_ref(function.id))
        .expect("writing to a string cannot fail");

    for (index, _) in function.params.iter().enumerate() {
        if index > 0 {
            source.push_str(", ");
        }
        source.push_str(&render_param_ref(ParamRef::new(index)));
    }

    source.push_str(") -> out {\n");

    match &function.body {
        FunctionBody::U256(body) => {
            render_stmts(source, &body.stmts, indent + 1);
            writeln!(source, "{}out := {}", indent_str(indent + 1), render_u256_value(body.result))
                .expect("writing to a string cannot fail");
        }
        FunctionBody::Bool(body) => {
            render_stmts(source, &body.stmts, indent + 1);
            writeln!(source, "{}out := {}", indent_str(indent + 1), render_bool_value(body.result))
                .expect("writing to a string cannot fail");
        }
    }

    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
}

fn render_stmts(source: &mut String, stmts: &[Stmt], indent: usize) {
    for stmt in stmts {
        render_stmt(source, stmt, indent);
    }
}

fn render_stmt(source: &mut String, stmt: &Stmt, indent: usize) {
    match stmt {
        Stmt::LetU256 { id, expr, .. } => {
            let target = render_u256_ref(*id);
            writeln!(source, "{}let {target} := 0", indent_str(indent))
                .expect("writing to a string cannot fail");
            render_u256_assign(source, expr, &target, indent);
        }
        Stmt::LetBool { id, expr, .. } => {
            let target = render_bool_ref(*id);
            writeln!(source, "{}let {target} := 0", indent_str(indent))
                .expect("writing to a string cannot fail");
            render_bool_assign(source, expr, &target, indent);
        }
        Stmt::AssignU256 { target, expr } => {
            render_u256_assign(source, expr, &render_u256_ref(*target), indent);
        }
        Stmt::AssignBool { target, expr } => {
            render_bool_assign(source, expr, &render_bool_ref(*target), indent);
        }
        Stmt::If { cond, then_block, else_block } => {
            render_conditional_stmt(
                source,
                render_bool_value(*cond),
                then_block,
                else_block,
                indent,
            );
        }
    }
}

fn render_conditional_stmt(
    source: &mut String,
    cond: String,
    then_block: &StmtBlock,
    else_block: &StmtBlock,
    indent: usize,
) {
    writeln!(source, "{}switch {cond}", indent_str(indent))
        .expect("writing to a string cannot fail");
    writeln!(source, "{}case 0 {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &else_block.stmts, indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
    writeln!(source, "{}default {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &then_block.stmts, indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
}

fn render_u256_assign(source: &mut String, expr: &U256Expr, target: &str, indent: usize) {
    match expr {
        U256Expr::Const(value) => render_assign(source, target, &value.render(), indent),
        U256Expr::Value(value) => render_assign(source, target, &render_u256_value(*value), indent),
        U256Expr::Unary { op, value } => render_assign(
            source,
            target,
            &format!("{}({})", yul_u256_unary_op(*op), render_u256_value(*value)),
            indent,
        ),
        U256Expr::Binary { op, left, right } => render_assign(
            source,
            target,
            &format!(
                "{}({}, {})",
                yul_u256_binary_op(*op),
                render_u256_value(*left),
                render_u256_value(*right)
            ),
            indent,
        ),
        U256Expr::Ternary { op, first, second, third } => render_assign(
            source,
            target,
            &format!(
                "{}({}, {}, {})",
                yul_u256_ternary_op(*op),
                render_u256_value(*first),
                render_u256_value(*second),
                render_u256_value(*third)
            ),
            indent,
        ),
        U256Expr::If { cond, then_block, else_block } => {
            render_u256_conditional_assign(
                source,
                render_bool_value(*cond),
                then_block,
                else_block,
                target,
                indent,
            );
        }
        U256Expr::Block(block) => {
            render_stmts(source, &block.stmts, indent);
            render_assign(source, target, &render_u256_value(block.result), indent);
        }
        U256Expr::Call { function, args } => {
            render_assign(source, target, &render_call(*function, args), indent);
        }
    }
}

fn render_bool_assign(source: &mut String, expr: &BoolExpr, target: &str, indent: usize) {
    match expr {
        BoolExpr::Const(value) => {
            render_assign(source, target, if *value { "1" } else { "0" }, indent)
        }
        BoolExpr::Value(value) => render_assign(source, target, &render_bool_value(*value), indent),
        BoolExpr::IsZero(value) => {
            render_assign(source, target, &format!("iszero({})", render_u256_value(*value)), indent)
        }
        BoolExpr::Compare { op, left, right } => render_assign(
            source,
            target,
            &format!(
                "{}({}, {})",
                yul_compare_op(*op),
                render_u256_value(*left),
                render_u256_value(*right)
            ),
            indent,
        ),
        BoolExpr::Binary { op, left, right } => render_assign(
            source,
            target,
            &format!(
                "{}({}, {})",
                yul_bool_binary_op(*op),
                render_bool_value(*left),
                render_bool_value(*right)
            ),
            indent,
        ),
        BoolExpr::If { cond, then_block, else_block } => {
            render_bool_conditional_assign(
                source,
                render_bool_value(*cond),
                then_block,
                else_block,
                target,
                indent,
            );
        }
        BoolExpr::Block(block) => {
            render_stmts(source, &block.stmts, indent);
            render_assign(source, target, &render_bool_value(block.result), indent);
        }
        BoolExpr::Call { function, args } => {
            render_assign(source, target, &render_call(*function, args), indent);
        }
    }
}

fn render_u256_conditional_assign(
    source: &mut String,
    cond: String,
    then_block: &Block<U256Value>,
    else_block: &Block<U256Value>,
    target: &str,
    indent: usize,
) {
    writeln!(source, "{}switch {cond}", indent_str(indent))
        .expect("writing to a string cannot fail");
    writeln!(source, "{}case 0 {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &else_block.stmts, indent + 1);
    render_assign(source, target, &render_u256_value(else_block.result), indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
    writeln!(source, "{}default {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &then_block.stmts, indent + 1);
    render_assign(source, target, &render_u256_value(then_block.result), indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
}

fn render_bool_conditional_assign(
    source: &mut String,
    cond: String,
    then_block: &Block<BoolValue>,
    else_block: &Block<BoolValue>,
    target: &str,
    indent: usize,
) {
    writeln!(source, "{}switch {cond}", indent_str(indent))
        .expect("writing to a string cannot fail");
    writeln!(source, "{}case 0 {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &else_block.stmts, indent + 1);
    render_assign(source, target, &render_bool_value(else_block.result), indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
    writeln!(source, "{}default {{", indent_str(indent)).expect("writing to a string cannot fail");
    render_stmts(source, &then_block.stmts, indent + 1);
    render_assign(source, target, &render_bool_value(then_block.result), indent + 1);
    writeln!(source, "{}}}", indent_str(indent)).expect("writing to a string cannot fail");
}

fn render_assign(source: &mut String, target: &str, value: &str, indent: usize) {
    writeln!(source, "{}{target} := {value}", indent_str(indent))
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

fn yul_u256_unary_op(op: U256UnaryOp) -> &'static str {
    match op {
        U256UnaryOp::Not => "not",
    }
}

fn yul_u256_binary_op(op: U256BinaryOp) -> &'static str {
    match op {
        U256BinaryOp::Add => "add",
        U256BinaryOp::Sub => "sub",
        U256BinaryOp::Mul => "mul",
        U256BinaryOp::Div => "div",
        U256BinaryOp::Mod => "mod",
        U256BinaryOp::And => "and",
        U256BinaryOp::Or => "or",
        U256BinaryOp::Xor => "xor",
        U256BinaryOp::Shl => "shl",
        U256BinaryOp::Shr => "shr",
        U256BinaryOp::Sar => "sar",
        U256BinaryOp::Byte => "byte",
    }
}

fn yul_u256_ternary_op(op: U256TernaryOp) -> &'static str {
    match op {
        U256TernaryOp::AddMod => "addmod",
        U256TernaryOp::MulMod => "mulmod",
    }
}

fn yul_compare_op(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "eq",
        CompareOp::Lt => "lt",
        CompareOp::Gt => "gt",
        CompareOp::SLt => "slt",
        CompareOp::SGt => "sgt",
    }
}

fn yul_bool_binary_op(op: BoolBinaryOp) -> &'static str {
    match op {
        BoolBinaryOp::And => "and",
        BoolBinaryOp::Or => "or",
        BoolBinaryOp::Xor => "xor",
        BoolBinaryOp::Eq => "eq",
    }
}

fn indent_str(indent: usize) -> String {
    INDENT.repeat(indent)
}

#[cfg(test)]
mod tests {
    use super::render_program;
    use crate::generator::ast::{
        Block, BoolExpr, BoolRef, BoolValue, CompareOp, Program, Stmt, U256BinaryOp, U256Const,
        U256Expr, U256Ref, U256Value,
    };

    #[test]
    fn renders_fallback_contract() {
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

        let source = render_program(&program);

        assert!(source.contains("contract C {"));
        assert!(source.contains("fallback() external payable"));
        assert!(source.contains("let in0 := calldataload(0)"));
        assert!(source.contains("v0 := add(in0, in1)"));
        assert!(source.contains("switch b0"));
        assert!(source.contains("return(0, 32)"));
    }
}
