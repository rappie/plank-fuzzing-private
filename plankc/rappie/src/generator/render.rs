use crate::generator::ast::{
    BoolExpr, BoolRef, BoolValue, Program, Stmt, U256Expr, U256Ref, U256Value,
};
use std::fmt::Write;

pub(crate) fn render_program(program: &Program) -> String {
    debug_assert!(program.validate().is_ok());

    let mut source = String::new();
    source.push_str("init {\n");

    for input in 0..program.input_words {
        let offset = u64::from(input) * 32;
        writeln!(source, "    let in{input} = @evm_calldataload({offset});")
            .expect("writing to a string cannot fail");
    }

    if !program.stmts.is_empty() {
        source.push('\n');
    }

    for stmt in &program.stmts {
        match stmt {
            Stmt::LetU256 { id, expr } => {
                writeln!(source, "    let {} = {};", render_u256_ref(*id), render_u256_expr(expr))
                    .expect("writing to a string cannot fail");
            }
            Stmt::LetBool { id, expr } => {
                writeln!(source, "    let {} = {};", render_bool_ref(*id), render_bool_expr(expr))
                    .expect("writing to a string cannot fail");
            }
        }
    }

    source.push_str("\n");
    source.push_str("    let out = @malloc_uninit(32);\n");
    writeln!(source, "    @mstore32(out, {});", render_u256_value(program.result))
        .expect("writing to a string cannot fail");
    source.push_str("    @evm_return(out, 32);\n");
    source.push_str("}\n");

    source
}

fn render_u256_expr(expr: &U256Expr) -> String {
    match expr {
        U256Expr::Const(value) => value.to_string(),
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
        U256Expr::If { cond, then_value, else_value } => {
            format!(
                "if {} {{ {} }} else {{ {} }}",
                render_bool_value(*cond),
                render_u256_value(*then_value),
                render_u256_value(*else_value)
            )
        }
    }
}

fn render_bool_expr(expr: &BoolExpr) -> String {
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
    }
}

fn render_u256_value(value: U256Value) -> String {
    match value {
        U256Value::Input(index) => format!("in{index}"),
        U256Value::Local(local) => render_u256_ref(local),
    }
}

fn render_bool_value(value: BoolValue) -> String {
    match value {
        BoolValue::Local(local) => render_bool_ref(local),
    }
}

fn render_u256_ref(local: U256Ref) -> String {
    format!("v{}", local.index())
}

fn render_bool_ref(local: BoolRef) -> String {
    format!("b{}", local.index())
}

#[cfg(test)]
mod tests {
    use super::render_program;
    use crate::generator::ast::{
        BoolRef, BoolValue, CompareOp, Program, Stmt, U256BinaryOp, U256Expr, U256Ref, U256Value,
    };

    #[test]
    fn renders_hand_constructed_program() {
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
                    expr: crate::generator::ast::BoolExpr::Compare {
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

        assert_eq!(
            render_program(&program),
            r#"init {
    let in0 = @evm_calldataload(0);
    let in1 = @evm_calldataload(32);

    let v0 = @evm_add(in0, in1);
    let b0 = @evm_lt(v0, in0);
    let v1 = if b0 { v0 } else { in1 };

    let out = @malloc_uninit(32);
    @mstore32(out, v1);
    @evm_return(out, 32);
}
"#
        );
    }
}
