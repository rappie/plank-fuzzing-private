use crate::expr::{Expr, render_expr};

pub(crate) fn render_program(expr: &Expr) -> String {
    format!(
        r#"
init {{
    let a = @evm_calldataload(0);
    let b = @evm_calldataload(32);
    let result = {};

    let out = @malloc_uninit(32);
    @mstore32(out, result);
    @evm_return(out, 32);
}}
"#,
        render_expr(expr)
    )
}
