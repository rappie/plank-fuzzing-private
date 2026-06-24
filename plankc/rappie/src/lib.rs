use alloy_primitives::{Bytes, hex};
pub use plank_driver::BackendKind;
use plank_driver::Driver;
use plank_evm::EvmVersion;
use plank_source::source_fs::InMemoryFs;
use revm::{
    ExecuteEvm, MainBuilder, MainContext,
    bytecode::Bytecode,
    context::{Context, TxEnv},
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::{Address, TxKind},
    state::AccountInfo,
};
use std::path::Path;

const MAIN_PATH: &str = "main.plk";
const TARGET: Address = Address::new([0xCC; 20]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmRunResult {
    pub success: bool,
    pub output: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Const(u64),
    CalldataWord0,
    CalldataWord1,
    Add(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
}

pub fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Const(value) => value.to_string(),
        Expr::CalldataWord0 => "a".to_string(),
        Expr::CalldataWord1 => "b".to_string(),
        Expr::Add(left, right) => format!("({} +% {})", render_expr(left), render_expr(right)),
        Expr::Xor(left, right) => format!("({} ^ {})", render_expr(left), render_expr(right)),
        Expr::And(left, right) => format!("({} & {})", render_expr(left), render_expr(right)),
    }
}

pub fn render_program(expr: &Expr) -> String {
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

pub fn compile_plank_source(source: &str, backend: BackendKind) -> Result<Vec<u8>, String> {
    let mut fs = InMemoryFs::new();
    fs.add_file(MAIN_PATH, source.to_string());

    let mut driver = Driver::new(&fs);
    let project =
        driver.load_project(Path::new(MAIN_PATH)).ok_or_else(|| render_diagnostics(&driver))?;

    let hir = driver.lower_hir(&project);
    let mir = driver.evaluate_hir(&hir, project.core_ops_source, EvmVersion::Osaka);
    if driver.session.has_errors() {
        return Err(render_diagnostics(&driver));
    }

    driver.emit_bytecode_with_backend(&mir, None, false, false, false, backend)
}

pub fn run_bytecode(bytecode: &[u8], calldata: &[u8]) -> EvmRunResult {
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(
        TARGET,
        AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::copy_from_slice(bytecode))),
    );

    let tx = TxEnv::builder()
        .kind(TxKind::Call(TARGET))
        .data(Bytes::copy_from_slice(calldata))
        .build()
        .expect("transaction environment should be valid");

    let result = Context::mainnet()
        .with_db(db)
        .build_mainnet()
        .transact(tx)
        .expect("EVM transaction should execute");

    EvmRunResult {
        success: result.result.is_success(),
        output: result.result.into_output().map_or_else(Vec::new, |output| output.to_vec()),
    }
}

#[track_caller]
pub fn assert_backends_match(source: &str, calldata: &[u8]) {
    let sir_debug = compile_or_panic(source, BackendKind::SirDebug);
    let sir_release = compile_or_panic(source, BackendKind::SirRelease);

    let sir_debug_result = run_bytecode(&sir_debug, calldata);
    let sir_release_result = run_bytecode(&sir_release, calldata);

    assert_same_result("sir-debug", &sir_debug_result, "sir-release", &sir_release_result);
}

#[track_caller]
pub fn assert_same_result(
    left_name: &str,
    left: &EvmRunResult,
    right_name: &str,
    right: &EvmRunResult,
) {
    assert_eq!(
        left.success, right.success,
        "success mismatch: {left_name}={} {right_name}={}",
        left.success, right.success
    );
    assert_eq!(
        left.output,
        right.output,
        "output mismatch:\n{left_name}: 0x{}\n{right_name}: 0x{}",
        hex::encode(&left.output),
        hex::encode(&right.output)
    );
}

#[track_caller]
fn compile_or_panic(source: &str, backend: BackendKind) -> Vec<u8> {
    compile_plank_source(source, backend).unwrap_or_else(|err| {
        panic!("compilation failed for {backend:?}:\n{err}\n\nsource:\n{source}")
    })
}

fn render_diagnostics<F: plank_source::SourceFs>(driver: &Driver<'_, F>) -> String {
    if driver.session.diagnostics().is_empty() {
        return "compilation failed without diagnostics".to_string();
    }

    driver
        .session
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.render_plain(&driver.session))
        .collect::<Vec<_>>()
        .join("\n----\n")
}
