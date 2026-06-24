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
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

pub fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Const(value) => value.to_string(),
        Expr::CalldataWord0 => "a".to_string(),
        Expr::CalldataWord1 => "b".to_string(),
        Expr::Add(left, right) => format!("({} +% {})", render_expr(left), render_expr(right)),
        Expr::Sub(left, right) => format!("({} -% {})", render_expr(left), render_expr(right)),
        Expr::Mul(left, right) => format!("({} *% {})", render_expr(left), render_expr(right)),
        Expr::Xor(left, right) => format!("({} ^ {})", render_expr(left), render_expr(right)),
        Expr::And(left, right) => format!("({} & {})", render_expr(left), render_expr(right)),
        Expr::Or(left, right) => format!("({} | {})", render_expr(left), render_expr(right)),
    }
}

pub fn generate_expr(seed: u64, max_depth: u8) -> Expr {
    let mut rng = SeededRng::new(seed);
    generate_expr_with_rng(&mut rng, max_depth)
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

#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub fn below(&mut self, upper: u64) -> u64 {
        assert!(upper > 0, "upper bound must be non-zero");
        self.next_u64() % upper
    }
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

pub fn backends_match(source: &str, calldata: &[u8]) -> Result<(), String> {
    let sir_debug = compile_plank_source(source, BackendKind::SirDebug)
        .map_err(|err| format!("sir-debug compilation failed:\n{err}"))?;
    let sir_release = compile_plank_source(source, BackendKind::SirRelease)
        .map_err(|err| format!("sir-release compilation failed:\n{err}"))?;

    let sir_debug_result = run_bytecode(&sir_debug, calldata);
    let sir_release_result = run_bytecode(&sir_release, calldata);

    same_result("sir-debug", &sir_debug_result, "sir-release", &sir_release_result)
}

#[track_caller]
pub fn assert_backends_match(source: &str, calldata: &[u8]) {
    backends_match(source, calldata)
        .unwrap_or_else(|err| panic!("backend comparison failed:\n{err}\n\nsource:\n{source}"));
}

#[track_caller]
pub fn assert_same_result(
    left_name: &str,
    left: &EvmRunResult,
    right_name: &str,
    right: &EvmRunResult,
) {
    same_result(left_name, left, right_name, right).unwrap_or_else(|err| panic!("{err}"));
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

fn generate_expr_with_rng(rng: &mut SeededRng, max_depth: u8) -> Expr {
    if max_depth == 0 {
        return generate_leaf(rng);
    }

    match rng.below(9) {
        0..=2 => generate_leaf(rng),
        3 => binary_expr(rng, max_depth, Expr::Add),
        4 => binary_expr(rng, max_depth, Expr::Sub),
        5 => binary_expr(rng, max_depth, Expr::Mul),
        6 => binary_expr(rng, max_depth, Expr::Xor),
        7 => binary_expr(rng, max_depth, Expr::And),
        8 => binary_expr(rng, max_depth, Expr::Or),
        _ => unreachable!("rng.below(9) returns 0..=8"),
    }
}

fn generate_leaf(rng: &mut SeededRng) -> Expr {
    match rng.below(3) {
        0 => Expr::Const(rng.next_u64() & 0xffff),
        1 => Expr::CalldataWord0,
        2 => Expr::CalldataWord1,
        _ => unreachable!("rng.below(3) returns 0..=2"),
    }
}

fn binary_expr(rng: &mut SeededRng, max_depth: u8, make: fn(Box<Expr>, Box<Expr>) -> Expr) -> Expr {
    let next_depth = max_depth - 1;
    make(
        Box::new(generate_expr_with_rng(rng, next_depth)),
        Box::new(generate_expr_with_rng(rng, next_depth)),
    )
}

fn same_result(
    left_name: &str,
    left: &EvmRunResult,
    right_name: &str,
    right: &EvmRunResult,
) -> Result<(), String> {
    if left.success != right.success {
        return Err(format!(
            "success mismatch: {left_name}={} {right_name}={}",
            left.success, right.success
        ));
    }

    if left.output != right.output {
        return Err(format!(
            "output mismatch:\n{left_name}: 0x{}\n{right_name}: 0x{}",
            hex::encode(&left.output),
            hex::encode(&right.output)
        ));
    }

    Ok(())
}
