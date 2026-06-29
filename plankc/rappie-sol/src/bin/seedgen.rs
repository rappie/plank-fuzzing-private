use alloy_primitives as _;
use arbitrary::{Arbitrary, Unstructured};
use plank_driver as _;
use plank_evm as _;
use plank_source as _;
use rappie_sol::{
    FuzzCase, SeedClassification, SeedExitKind, SeedProgramMode, execute_plank_solidity,
};
use revm as _;
use serde as _;
use serde_json as _;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    fmt::Write,
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};
use toml as _;

const DEFAULT_TARGET: usize = 320;
const DEFAULT_CANDIDATE_LIMIT: usize = 300_000;
const DEFAULT_OUTPUT_DIR: &str = "fuzz/seeds/plank_sol_program_diff";
const SIZES: [usize; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

const ALL_BUCKETS: &[Bucket] = &[
    Bucket::RawFallback,
    Bucket::SelectorDispatch,
    Bucket::DispatchMax,
    Bucket::MultiCall,
    Bucket::MaxCallCount,
    Bucket::MultiEntryTouched,
    Bucket::ShortCalldata,
    Bucket::UnalignedCalldata,
    Bucket::FullWidthWord,
    Bucket::Arithmetic,
    Bucket::SignedArithmetic,
    Bucket::MemoryWidth,
    Bucket::MemoryCopy,
    Bucket::CalldataCopy,
    Bucket::Storage,
    Bucket::RepeatedStorageSlot,
    Bucket::TransientStorage,
    Bucket::ExternalCode,
    Bucket::Call,
    Bucket::DelegateCall,
    Bucket::Returndata,
    Bucket::Create,
    Bucket::Create2,
    Bucket::Log,
    Bucket::Log4,
    Bucket::Branch,
    Bucket::Loop,
    Bucket::ReturnExit,
    Bucket::RevertExit,
    Bucket::ConditionalExit,
    Bucket::StopExit,
    Bucket::InvalidExit,
    Bucket::StorageAndMultiCall,
    Bucket::CallAndReturndata,
    Bucket::CreateAndExternalCode,
    Bucket::Struct,
    Bucket::Tuple,
    Bucket::CompoundLiteral,
    Bucket::FieldAccess,
    Bucket::FieldUpdate,
    Bucket::ComptimeTypeReflection,
    Bucket::CBytesBuiltin,
    Bucket::HighLevelOperator,
    Bucket::CoreOpsOperator,
    Bucket::HelperFunction,
    Bucket::NestedHelperCall,
    Bucket::Import,
    Bucket::ImportSingle,
    Bucket::ImportGroup,
    Bucket::ImportAlias,
    Bucket::ImportGlob,
    Bucket::DeepImport,
    Bucket::Comments,
    Bucket::BinaryLiteral,
    Bucket::HexLiteral,
    Bucket::ParameterizedType,
    Bucket::ComptimeControlFlow,
    Bucket::ComptimeLoop,
    Bucket::TypeDependentBranch,
    Bucket::FunctionReturnsCompound,
    Bucket::FunctionEarlyReturn,
    Bucket::NestedCompound,
    Bucket::RuntimeUninit,
    Bucket::DataOffset,
    Bucket::GenericInferredParam,
    Bucket::ComptimeValueParam,
    Bucket::ComptimeCompoundParam,
    Bucket::FunctionValuedComptime,
    Bucket::CBytesEdgeShapes,
    Bucket::DataOffsetDedup,
    Bucket::DataOffsetConcat,
    Bucket::AnonymousTypeIdentity,
    Bucket::CrossFileTypeIdentity,
    Bucket::EvalBranchQuota,
    Bucket::EmptyCompound,
    Bucket::ComptimeOnlyCompound,
    Bucket::RuntimeOnlyCompound,
    Bucket::StdRegistered,
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let config = Config::parse()?;
    if config.help {
        print_help();
        return Ok(());
    }

    if config.target < ALL_BUCKETS.len() {
        return Err(format!(
            "target {} is smaller than the {} required buckets",
            config.target,
            ALL_BUCKETS.len()
        ));
    }

    let fixed = fixed_candidates();
    let mut covered = BTreeSet::new();
    let mut seen_combos = HashSet::new();
    let mut selected = Vec::with_capacity(config.target);
    let mut name_counts = BTreeMap::new();
    let mut stats = Stats::default();

    for candidate_index in 0..config.candidate_limit {
        stats.generated += 1;
        let bytes = candidate_bytes(candidate_index, &fixed);
        let Some(case) = decode_case(&bytes) else {
            continue;
        };
        stats.decoded += 1;

        let classification = case.seed_classification();
        let buckets = buckets_for(&classification);
        let combo = ComboKey::from(classification);
        let has_new_bucket = buckets.iter().any(|bucket| !covered.contains(bucket));
        let has_new_combo = !seen_combos.contains(&combo);

        if !has_new_bucket && (selected.len() >= config.target || !has_new_combo) {
            continue;
        }

        if config.verify && !verified(&case) {
            stats.rejected += 1;
            continue;
        }
        stats.verified += 1;

        let new_buckets =
            buckets.iter().copied().filter(|bucket| !covered.contains(bucket)).collect::<Vec<_>>();
        let has_new_combo = seen_combos.insert(combo);

        if new_buckets.is_empty() && (selected.len() >= config.target || !has_new_combo) {
            continue;
        }

        let name_bucket = new_buckets.first().copied().unwrap_or(Bucket::ComboExtra);
        let name = next_seed_name(name_bucket, &mut name_counts);
        covered.extend(buckets.iter().copied());
        selected.push(SelectedSeed { name, bytes, classification, buckets });

        if selected.len() >= config.target && covered.len() == ALL_BUCKETS.len() {
            break;
        }
    }

    let selected = prune_selected(selected, config.target);
    let covered = covered_buckets(&selected);
    let missing =
        ALL_BUCKETS.iter().copied().filter(|bucket| !covered.contains(bucket)).collect::<Vec<_>>();

    if selected.len() < config.target {
        return Err(format!(
            "selected only {} seeds, target was {}; missing buckets: {:?}",
            selected.len(),
            config.target,
            missing
        ));
    }
    if !missing.is_empty() {
        return Err(format!("missing required seed buckets: {missing:?}"));
    }
    if selected.len() > config.target {
        return Err(format!(
            "selected {} seeds after pruning, target was {}",
            selected.len(),
            config.target
        ));
    }

    rewrite_seed_dir(&config.output_dir, &selected)
        .map_err(|err| format!("failed to write seeds: {err}"))?;
    write_manifest(&config.output_dir, &selected)
        .map_err(|err| format!("failed to write seed manifest: {err}"))?;

    println!(
        "wrote {} seeds to {} from {} generated candidates ({} decoded, {} verified, {} rejected)",
        selected.len(),
        config.output_dir.display(),
        stats.generated,
        stats.decoded,
        stats.verified,
        stats.rejected,
    );
    for seed in &selected {
        println!(
            "{}: {:?}, mode={:?}, entries={}, calls={}, exit={:?}",
            seed.name,
            seed.buckets,
            seed.classification.mode,
            seed.classification.entry_count,
            seed.classification.call_count,
            seed.classification.exit_kind
        );
    }

    Ok(())
}

#[derive(Debug)]
struct Config {
    output_dir: PathBuf,
    target: usize,
    candidate_limit: usize,
    verify: bool,
    help: bool,
}

impl Config {
    fn parse() -> Result<Self, String> {
        let mut config = Self {
            output_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_OUTPUT_DIR),
            target: DEFAULT_TARGET,
            candidate_limit: DEFAULT_CANDIDATE_LIMIT,
            verify: true,
            help: false,
        };
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--output-dir" => {
                    config.output_dir = PathBuf::from(
                        args.next().ok_or_else(|| "--output-dir requires a path".to_string())?,
                    );
                }
                "--target" => {
                    config.target = args
                        .next()
                        .ok_or_else(|| "--target requires a number".to_string())?
                        .parse()
                        .map_err(|err| format!("invalid --target: {err}"))?;
                }
                "--candidate-limit" => {
                    config.candidate_limit = args
                        .next()
                        .ok_or_else(|| "--candidate-limit requires a number".to_string())?
                        .parse()
                        .map_err(|err| format!("invalid --candidate-limit: {err}"))?;
                }
                "--no-verify" => config.verify = false,
                "--help" | "-h" => config.help = true,
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        Ok(config)
    }
}

#[derive(Debug, Default)]
struct Stats {
    generated: usize,
    decoded: usize,
    verified: usize,
    rejected: usize,
}

#[derive(Debug)]
struct SelectedSeed {
    name: String,
    bytes: Vec<u8>,
    classification: SeedClassification,
    buckets: Vec<Bucket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ComboKey {
    mode: SeedProgramMode,
    entry_count: usize,
    call_count: usize,
    exit_kind: SeedExitKind,
    max_log_topics: usize,
    flags: u128,
}

impl From<SeedClassification> for ComboKey {
    fn from(classification: SeedClassification) -> Self {
        let mut flags = 0u128;
        let bools = [
            classification.touches_multiple_entries,
            classification.has_short_calldata,
            classification.has_unaligned_calldata,
            classification.has_full_width_word,
            classification.has_arithmetic,
            classification.has_signed_arithmetic,
            classification.has_memory_width,
            classification.has_memory_copy,
            classification.has_calldata_copy,
            classification.has_storage,
            classification.has_repeated_storage_slot,
            classification.has_transient_storage,
            classification.has_external_code,
            classification.has_call,
            classification.has_delegatecall,
            classification.has_returndata,
            classification.has_create,
            classification.has_create2,
            classification.has_log,
            classification.has_branch,
            classification.has_loop,
            classification.has_struct,
            classification.has_tuple,
            classification.has_compound_literal,
            classification.has_field_access,
            classification.has_field_update,
            classification.has_comptime_type_reflection,
            classification.has_cbytes_builtin,
            classification.has_high_level_operator,
            classification.has_core_ops_operator,
            classification.has_helper_function,
            classification.has_nested_helper_call,
            classification.has_import,
            classification.has_import_single,
            classification.has_import_group,
            classification.has_import_alias,
            classification.has_import_glob,
            classification.has_deep_import,
            classification.has_comments,
            classification.has_binary_literal,
            classification.has_hex_literal,
            classification.has_parameterized_type,
            classification.has_comptime_control_flow,
            classification.has_comptime_loop,
            classification.has_type_dependent_branch,
            classification.has_function_returns_compound,
            classification.has_function_early_return,
            classification.has_nested_compound,
            classification.has_runtime_uninit,
            classification.has_data_offset,
            classification.has_generic_inferred_param,
            classification.has_comptime_value_param,
            classification.has_comptime_compound_param,
            classification.has_function_valued_comptime,
            classification.has_cbytes_edge_shapes,
            classification.has_data_offset_dedup,
            classification.has_data_offset_concat,
            classification.has_anonymous_type_identity,
            classification.has_cross_file_type_identity,
            classification.has_eval_branch_quota,
            classification.has_empty_compound,
            classification.has_comptime_only_compound,
            classification.has_runtime_only_compound,
            classification.has_std_registered,
        ];

        for (index, active) in bools.into_iter().enumerate() {
            if active {
                flags |= 1u128 << index;
            }
        }

        Self {
            mode: classification.mode,
            entry_count: classification.entry_count,
            call_count: classification.call_count,
            exit_kind: classification.exit_kind,
            max_log_topics: classification.max_log_topics,
            flags,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Bucket {
    RawFallback,
    SelectorDispatch,
    DispatchMax,
    MultiCall,
    MaxCallCount,
    MultiEntryTouched,
    ShortCalldata,
    UnalignedCalldata,
    FullWidthWord,
    Arithmetic,
    SignedArithmetic,
    MemoryWidth,
    MemoryCopy,
    CalldataCopy,
    Storage,
    RepeatedStorageSlot,
    TransientStorage,
    ExternalCode,
    Call,
    DelegateCall,
    Returndata,
    Create,
    Create2,
    Log,
    Log4,
    Branch,
    Loop,
    ReturnExit,
    RevertExit,
    ConditionalExit,
    StopExit,
    InvalidExit,
    StorageAndMultiCall,
    CallAndReturndata,
    CreateAndExternalCode,
    Struct,
    Tuple,
    CompoundLiteral,
    FieldAccess,
    FieldUpdate,
    ComptimeTypeReflection,
    CBytesBuiltin,
    HighLevelOperator,
    CoreOpsOperator,
    HelperFunction,
    NestedHelperCall,
    Import,
    ImportSingle,
    ImportGroup,
    ImportAlias,
    ImportGlob,
    DeepImport,
    Comments,
    BinaryLiteral,
    HexLiteral,
    ParameterizedType,
    ComptimeControlFlow,
    ComptimeLoop,
    TypeDependentBranch,
    FunctionReturnsCompound,
    FunctionEarlyReturn,
    NestedCompound,
    RuntimeUninit,
    DataOffset,
    GenericInferredParam,
    ComptimeValueParam,
    ComptimeCompoundParam,
    FunctionValuedComptime,
    CBytesEdgeShapes,
    DataOffsetDedup,
    DataOffsetConcat,
    AnonymousTypeIdentity,
    CrossFileTypeIdentity,
    EvalBranchQuota,
    EmptyCompound,
    ComptimeOnlyCompound,
    RuntimeOnlyCompound,
    StdRegistered,
    ComboExtra,
}

fn print_help() {
    println!(
        "Usage: cargo run -p rappie-sol --bin seedgen -- [--output-dir PATH] [--target N] [--candidate-limit N] [--no-verify]"
    );
}

fn decode_case(bytes: &[u8]) -> Option<FuzzCase> {
    let mut unstructured = Unstructured::new(bytes);
    FuzzCase::arbitrary(&mut unstructured).ok()
}

fn verified(case: &FuzzCase) -> bool {
    let Ok(executions) = execute_plank_solidity(case) else {
        return false;
    };

    executions
        .solidity_peers
        .iter()
        .chain(&executions.plank_backends)
        .all(|execution| execution.trace == executions.reference.trace)
}

fn buckets_for(classification: &SeedClassification) -> Vec<Bucket> {
    let mut buckets = Vec::new();

    match classification.mode {
        SeedProgramMode::RawFallback => buckets.push(Bucket::RawFallback),
        SeedProgramMode::SelectorDispatch => buckets.push(Bucket::SelectorDispatch),
    }

    if classification.entry_count >= 6 {
        buckets.push(Bucket::DispatchMax);
    }
    if classification.call_count > 1 {
        buckets.push(Bucket::MultiCall);
    }
    if classification.call_count >= 4 {
        buckets.push(Bucket::MaxCallCount);
    }
    if classification.touches_multiple_entries {
        buckets.push(Bucket::MultiEntryTouched);
    }
    if classification.has_short_calldata {
        buckets.push(Bucket::ShortCalldata);
    }
    if classification.has_unaligned_calldata {
        buckets.push(Bucket::UnalignedCalldata);
    }
    if classification.has_full_width_word {
        buckets.push(Bucket::FullWidthWord);
    }
    if classification.has_arithmetic {
        buckets.push(Bucket::Arithmetic);
    }
    if classification.has_signed_arithmetic {
        buckets.push(Bucket::SignedArithmetic);
    }
    if classification.has_memory_width {
        buckets.push(Bucket::MemoryWidth);
    }
    if classification.has_memory_copy {
        buckets.push(Bucket::MemoryCopy);
    }
    if classification.has_calldata_copy {
        buckets.push(Bucket::CalldataCopy);
    }
    if classification.has_storage {
        buckets.push(Bucket::Storage);
    }
    if classification.has_repeated_storage_slot {
        buckets.push(Bucket::RepeatedStorageSlot);
    }
    if classification.has_transient_storage {
        buckets.push(Bucket::TransientStorage);
    }
    if classification.has_external_code {
        buckets.push(Bucket::ExternalCode);
    }
    if classification.has_call {
        buckets.push(Bucket::Call);
    }
    if classification.has_delegatecall {
        buckets.push(Bucket::DelegateCall);
    }
    if classification.has_returndata {
        buckets.push(Bucket::Returndata);
    }
    if classification.has_create {
        buckets.push(Bucket::Create);
    }
    if classification.has_create2 {
        buckets.push(Bucket::Create2);
    }
    if classification.has_log {
        buckets.push(Bucket::Log);
    }
    if classification.max_log_topics == 4 {
        buckets.push(Bucket::Log4);
    }
    if classification.has_branch {
        buckets.push(Bucket::Branch);
    }
    if classification.has_loop {
        buckets.push(Bucket::Loop);
    }
    if classification.has_return_exit {
        buckets.push(Bucket::ReturnExit);
    }
    if classification.has_revert_exit {
        buckets.push(Bucket::RevertExit);
    }
    if classification.has_conditional_exit {
        buckets.push(Bucket::ConditionalExit);
    }
    if classification.has_stop_exit {
        buckets.push(Bucket::StopExit);
    }
    if classification.has_invalid_exit {
        buckets.push(Bucket::InvalidExit);
    }
    if classification.has_storage && classification.call_count > 1 {
        buckets.push(Bucket::StorageAndMultiCall);
    }
    if classification.has_call && classification.has_returndata {
        buckets.push(Bucket::CallAndReturndata);
    }
    if classification.has_external_code && (classification.has_create || classification.has_create2)
    {
        buckets.push(Bucket::CreateAndExternalCode);
    }
    if classification.has_struct {
        buckets.push(Bucket::Struct);
    }
    if classification.has_tuple {
        buckets.push(Bucket::Tuple);
    }
    if classification.has_compound_literal {
        buckets.push(Bucket::CompoundLiteral);
    }
    if classification.has_field_access {
        buckets.push(Bucket::FieldAccess);
    }
    if classification.has_field_update {
        buckets.push(Bucket::FieldUpdate);
    }
    if classification.has_comptime_type_reflection {
        buckets.push(Bucket::ComptimeTypeReflection);
    }
    if classification.has_cbytes_builtin {
        buckets.push(Bucket::CBytesBuiltin);
    }
    if classification.has_high_level_operator {
        buckets.push(Bucket::HighLevelOperator);
    }
    if classification.has_core_ops_operator {
        buckets.push(Bucket::CoreOpsOperator);
    }
    if classification.has_helper_function {
        buckets.push(Bucket::HelperFunction);
    }
    if classification.has_nested_helper_call {
        buckets.push(Bucket::NestedHelperCall);
    }
    if classification.has_import {
        buckets.push(Bucket::Import);
    }
    if classification.has_import_single {
        buckets.push(Bucket::ImportSingle);
    }
    if classification.has_import_group {
        buckets.push(Bucket::ImportGroup);
    }
    if classification.has_import_alias {
        buckets.push(Bucket::ImportAlias);
    }
    if classification.has_import_glob {
        buckets.push(Bucket::ImportGlob);
    }
    if classification.has_deep_import {
        buckets.push(Bucket::DeepImport);
    }
    if classification.has_comments {
        buckets.push(Bucket::Comments);
    }
    if classification.has_binary_literal {
        buckets.push(Bucket::BinaryLiteral);
    }
    if classification.has_hex_literal {
        buckets.push(Bucket::HexLiteral);
    }
    if classification.has_parameterized_type {
        buckets.push(Bucket::ParameterizedType);
    }
    if classification.has_comptime_control_flow {
        buckets.push(Bucket::ComptimeControlFlow);
    }
    if classification.has_comptime_loop {
        buckets.push(Bucket::ComptimeLoop);
    }
    if classification.has_type_dependent_branch {
        buckets.push(Bucket::TypeDependentBranch);
    }
    if classification.has_function_returns_compound {
        buckets.push(Bucket::FunctionReturnsCompound);
    }
    if classification.has_function_early_return {
        buckets.push(Bucket::FunctionEarlyReturn);
    }
    if classification.has_nested_compound {
        buckets.push(Bucket::NestedCompound);
    }
    if classification.has_runtime_uninit {
        buckets.push(Bucket::RuntimeUninit);
    }
    if classification.has_data_offset {
        buckets.push(Bucket::DataOffset);
    }
    if classification.has_generic_inferred_param {
        buckets.push(Bucket::GenericInferredParam);
    }
    if classification.has_comptime_value_param {
        buckets.push(Bucket::ComptimeValueParam);
    }
    if classification.has_comptime_compound_param {
        buckets.push(Bucket::ComptimeCompoundParam);
    }
    if classification.has_function_valued_comptime {
        buckets.push(Bucket::FunctionValuedComptime);
    }
    if classification.has_cbytes_edge_shapes {
        buckets.push(Bucket::CBytesEdgeShapes);
    }
    if classification.has_data_offset_dedup {
        buckets.push(Bucket::DataOffsetDedup);
    }
    if classification.has_data_offset_concat {
        buckets.push(Bucket::DataOffsetConcat);
    }
    if classification.has_anonymous_type_identity {
        buckets.push(Bucket::AnonymousTypeIdentity);
    }
    if classification.has_cross_file_type_identity {
        buckets.push(Bucket::CrossFileTypeIdentity);
    }
    if classification.has_eval_branch_quota {
        buckets.push(Bucket::EvalBranchQuota);
    }
    if classification.has_empty_compound {
        buckets.push(Bucket::EmptyCompound);
    }
    if classification.has_comptime_only_compound {
        buckets.push(Bucket::ComptimeOnlyCompound);
    }
    if classification.has_runtime_only_compound {
        buckets.push(Bucket::RuntimeOnlyCompound);
    }
    if classification.has_std_registered {
        buckets.push(Bucket::StdRegistered);
    }

    buckets
}

fn next_seed_name(bucket: Bucket, counts: &mut BTreeMap<&'static str, usize>) -> String {
    let prefix = bucket_name(bucket);
    let count = counts.entry(prefix).or_default();
    let name = format!("{prefix}_{count:02}");
    *count += 1;
    name
}

fn prune_selected(mut selected: Vec<SelectedSeed>, target: usize) -> Vec<SelectedSeed> {
    while selected.len() > target {
        let counts = bucket_counts(&selected);
        let Some(index) = selected.iter().rposition(|seed| {
            seed.buckets.iter().all(|bucket| {
                !ALL_BUCKETS.contains(bucket) || counts.get(bucket).copied().unwrap_or(0) > 1
            })
        }) else {
            break;
        };
        selected.remove(index);
    }

    selected
}

fn covered_buckets(selected: &[SelectedSeed]) -> BTreeSet<Bucket> {
    selected
        .iter()
        .flat_map(|seed| seed.buckets.iter().copied())
        .filter(|bucket| ALL_BUCKETS.contains(bucket))
        .collect()
}

fn bucket_counts(selected: &[SelectedSeed]) -> BTreeMap<Bucket, usize> {
    let mut counts = BTreeMap::new();
    for bucket in selected.iter().flat_map(|seed| seed.buckets.iter().copied()) {
        if ALL_BUCKETS.contains(&bucket) {
            *counts.entry(bucket).or_default() += 1;
        }
    }
    counts
}

fn bucket_name(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::RawFallback => "raw_fallback",
        Bucket::SelectorDispatch => "dispatch",
        Bucket::DispatchMax => "dispatch_max",
        Bucket::MultiCall => "multi_call",
        Bucket::MaxCallCount => "max_call_count",
        Bucket::MultiEntryTouched => "multi_entry",
        Bucket::ShortCalldata => "short_calldata",
        Bucket::UnalignedCalldata => "unaligned_calldata",
        Bucket::FullWidthWord => "full_width_word",
        Bucket::Arithmetic => "arithmetic",
        Bucket::SignedArithmetic => "signed_arithmetic",
        Bucket::MemoryWidth => "memory_width",
        Bucket::MemoryCopy => "memory_copy",
        Bucket::CalldataCopy => "calldata_copy",
        Bucket::Storage => "storage",
        Bucket::RepeatedStorageSlot => "repeated_storage",
        Bucket::TransientStorage => "transient_storage",
        Bucket::ExternalCode => "external_code",
        Bucket::Call => "call",
        Bucket::DelegateCall => "delegate_call",
        Bucket::Returndata => "returndata",
        Bucket::Create => "create",
        Bucket::Create2 => "create2",
        Bucket::Log => "log",
        Bucket::Log4 => "log4",
        Bucket::Branch => "branch",
        Bucket::Loop => "loop",
        Bucket::ReturnExit => "return_exit",
        Bucket::RevertExit => "revert_exit",
        Bucket::ConditionalExit => "conditional_exit",
        Bucket::StopExit => "stop_exit",
        Bucket::InvalidExit => "invalid_exit",
        Bucket::StorageAndMultiCall => "storage_multi_call",
        Bucket::CallAndReturndata => "call_returndata",
        Bucket::CreateAndExternalCode => "create_external_code",
        Bucket::Struct => "struct",
        Bucket::Tuple => "tuple",
        Bucket::CompoundLiteral => "compound_literal",
        Bucket::FieldAccess => "field_access",
        Bucket::FieldUpdate => "field_update",
        Bucket::ComptimeTypeReflection => "comptime_type_reflection",
        Bucket::CBytesBuiltin => "cbytes_builtin",
        Bucket::HighLevelOperator => "high_level_operator",
        Bucket::CoreOpsOperator => "core_ops_operator",
        Bucket::HelperFunction => "helper_function",
        Bucket::NestedHelperCall => "nested_helper_call",
        Bucket::Import => "import",
        Bucket::ImportSingle => "import_single",
        Bucket::ImportGroup => "import_group",
        Bucket::ImportAlias => "import_alias",
        Bucket::ImportGlob => "import_glob",
        Bucket::DeepImport => "deep_import",
        Bucket::Comments => "comments",
        Bucket::BinaryLiteral => "binary_literal",
        Bucket::HexLiteral => "hex_literal",
        Bucket::ParameterizedType => "parameterized_type",
        Bucket::ComptimeControlFlow => "comptime_control_flow",
        Bucket::ComptimeLoop => "comptime_loop",
        Bucket::TypeDependentBranch => "type_dependent_branch",
        Bucket::FunctionReturnsCompound => "function_returns_compound",
        Bucket::FunctionEarlyReturn => "function_early_return",
        Bucket::NestedCompound => "nested_compound",
        Bucket::RuntimeUninit => "runtime_uninit",
        Bucket::DataOffset => "data_offset",
        Bucket::GenericInferredParam => "generic_inferred_param",
        Bucket::ComptimeValueParam => "comptime_value_param",
        Bucket::ComptimeCompoundParam => "comptime_compound_param",
        Bucket::FunctionValuedComptime => "function_valued_comptime",
        Bucket::CBytesEdgeShapes => "cbytes_edge_shapes",
        Bucket::DataOffsetDedup => "data_offset_dedup",
        Bucket::DataOffsetConcat => "data_offset_concat",
        Bucket::AnonymousTypeIdentity => "anonymous_type_identity",
        Bucket::CrossFileTypeIdentity => "cross_file_type_identity",
        Bucket::EvalBranchQuota => "eval_branch_quota",
        Bucket::EmptyCompound => "empty_compound",
        Bucket::ComptimeOnlyCompound => "comptime_only_compound",
        Bucket::RuntimeOnlyCompound => "runtime_only_compound",
        Bucket::StdRegistered => "std_registered",
        Bucket::ComboExtra => "combo",
    }
}

fn fixed_candidates() -> Vec<Vec<u8>> {
    let mut candidates = Vec::new();

    for size in SIZES {
        candidates.push(vec![0x00; size]);
        candidates.push(vec![0x01; size]);
        candidates.push(vec![0xff; size]);
        candidates.push(vec![0x55; size]);
        candidates.push(vec![0xaa; size]);
        candidates.push(repeated_pattern(size, b"RappieSolTraceSeed"));
        candidates.push(repeated_pattern(size, b"DispatchStorageCallsCreateLogs"));
        candidates.push((0..size).map(|index| index as u8).collect());
        candidates.push((0..size).map(|index| 255u8.wrapping_sub(index as u8)).collect());
    }

    candidates
}

fn candidate_bytes(index: usize, fixed: &[Vec<u8>]) -> Vec<u8> {
    if let Some(candidate) = fixed.get(index) {
        return candidate.clone();
    }

    let random_index = index - fixed.len();
    let mut rng = XorShift64::new(0x9e37_79b9_7f4a_7c15 ^ random_index as u64);
    let size = SIZES[(rng.next_u64() as usize) % SIZES.len()];
    match rng.next_u64() % 5 {
        0 => random_bytes(size, &mut rng),
        1 => word_pattern_bytes(size, &mut rng),
        2 => sparse_edge_bytes(size, &mut rng),
        3 => mixed_pattern_bytes(size, &mut rng),
        _ => structured_pressure_bytes(size, &mut rng),
    }
}

fn random_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    (0..size).map(|_| rng.next_u64() as u8).collect()
}

fn word_pattern_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    const WORDS: [u64; 8] = [
        0,
        1,
        31,
        32,
        u64::MAX,
        0x5555_5555_5555_5555,
        0xaaaa_aaaa_aaaa_aaaa,
        0x8000_0000_0000_0000,
    ];

    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let word = WORDS[(rng.next_u64() as usize) % WORDS.len()];
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.truncate(size);
    bytes
}

fn sparse_edge_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    let mut bytes = vec![0; size];
    for byte in &mut bytes {
        let roll = rng.next_u64() % 18;
        *byte = match roll {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 5,
            6 => 6,
            7 => 31,
            8 => 32,
            9 => 33,
            10 => 64,
            11 => 96,
            12 => 128,
            13 => 160,
            14 => 192,
            15 => 255,
            _ => rng.next_u64() as u8,
        };
    }
    bytes
}

fn mixed_pattern_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    let pattern = match rng.next_u64() % 4 {
        0 => b"SelectorDispatchLogsStorageCalls".as_slice(),
        1 => b"ReturnRevertDynamicLoopMemory".as_slice(),
        2 => b"ExternalCodeCreateTransientReturndata".as_slice(),
        _ => b"\x00\x01\x04\x06\x08\x0a\x1f\x20\x21\x40\x60\x80\xc0\xff".as_slice(),
    };
    repeated_pattern(size, pattern)
}

fn structured_pressure_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size);
    let mut selector_bias = 0u8;
    while bytes.len() < size {
        bytes.push((rng.next_u64() as u8) % 6);
        bytes.push(selector_bias);
        bytes.push(4);
        bytes.push(10);
        bytes.extend_from_slice(&(rng.next_u64()).to_le_bytes());
        selector_bias = selector_bias.wrapping_add(1);
    }
    bytes.truncate(size);
    bytes
}

fn repeated_pattern(size: usize, pattern: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        bytes.extend_from_slice(pattern);
    }
    bytes.truncate(size);
    bytes
}

fn rewrite_seed_dir(output_dir: &Path, selected: &[SelectedSeed]) -> io::Result<()> {
    fs::create_dir_all(output_dir)?;
    for entry in fs::read_dir(output_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }

    for seed in selected {
        fs::write(output_dir.join(&seed.name), &seed.bytes)?;
    }

    Ok(())
}

fn write_manifest(output_dir: &Path, selected: &[SelectedSeed]) -> io::Result<()> {
    let manifest_path = output_dir.with_extension("manifest.txt");
    let mut manifest = String::new();
    writeln!(&mut manifest, "seed_count={}\noutput_dir={}\n", selected.len(), output_dir.display())
        .expect("writing to string cannot fail");

    for seed in selected {
        writeln!(
            &mut manifest,
            "{}: buckets={:?}; mode={:?}; entries={}; calls={}; exit={:?}",
            seed.name,
            seed.buckets,
            seed.classification.mode,
            seed.classification.entry_count,
            seed.classification.call_count,
            seed.classification.exit_kind
        )
        .expect("writing to string cannot fail");
    }

    fs::write(manifest_path, manifest)
}

#[derive(Debug, Clone, Copy)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_frontend_buckets_are_selected() {
        let mut classification = empty_classification();
        classification.has_parameterized_type = true;
        classification.has_import = true;
        classification.has_import_single = true;
        classification.has_import_group = true;
        classification.has_import_alias = true;
        classification.has_import_glob = true;
        classification.has_deep_import = true;
        classification.has_comptime_control_flow = true;
        classification.has_comptime_loop = true;
        classification.has_type_dependent_branch = true;
        classification.has_function_returns_compound = true;
        classification.has_function_early_return = true;
        classification.has_nested_compound = true;
        classification.has_runtime_uninit = true;
        classification.has_data_offset = true;
        classification.has_generic_inferred_param = true;
        classification.has_comptime_value_param = true;
        classification.has_comptime_compound_param = true;
        classification.has_function_valued_comptime = true;
        classification.has_cbytes_edge_shapes = true;
        classification.has_data_offset_dedup = true;
        classification.has_data_offset_concat = true;
        classification.has_anonymous_type_identity = true;
        classification.has_cross_file_type_identity = true;
        classification.has_eval_branch_quota = true;
        classification.has_empty_compound = true;
        classification.has_comptime_only_compound = true;
        classification.has_runtime_only_compound = true;

        let buckets = buckets_for(&classification);
        for bucket in [
            Bucket::ParameterizedType,
            Bucket::ImportSingle,
            Bucket::ImportGroup,
            Bucket::ImportAlias,
            Bucket::ImportGlob,
            Bucket::DeepImport,
            Bucket::ComptimeControlFlow,
            Bucket::ComptimeLoop,
            Bucket::TypeDependentBranch,
            Bucket::FunctionReturnsCompound,
            Bucket::FunctionEarlyReturn,
            Bucket::NestedCompound,
            Bucket::RuntimeUninit,
            Bucket::DataOffset,
            Bucket::GenericInferredParam,
            Bucket::ComptimeValueParam,
            Bucket::ComptimeCompoundParam,
            Bucket::FunctionValuedComptime,
            Bucket::CBytesEdgeShapes,
            Bucket::DataOffsetDedup,
            Bucket::DataOffsetConcat,
            Bucket::AnonymousTypeIdentity,
            Bucket::CrossFileTypeIdentity,
            Bucket::EvalBranchQuota,
            Bucket::EmptyCompound,
            Bucket::ComptimeOnlyCompound,
            Bucket::RuntimeOnlyCompound,
        ] {
            assert!(buckets.contains(&bucket), "missing bucket {bucket:?}");
        }
    }

    #[test]
    fn required_bucket_names_are_unique() {
        assert_eq!(DEFAULT_TARGET, 320);

        let mut names = BTreeSet::new();
        for bucket in ALL_BUCKETS {
            let name = bucket_name(*bucket);
            assert!(!name.is_empty());
            assert!(names.insert(name), "duplicate bucket name {name}");
        }
    }

    fn empty_classification() -> SeedClassification {
        SeedClassification {
            mode: SeedProgramMode::RawFallback,
            entry_count: 1,
            call_count: 1,
            touches_multiple_entries: false,
            has_short_calldata: false,
            has_unaligned_calldata: false,
            has_full_width_word: false,
            has_arithmetic: false,
            has_signed_arithmetic: false,
            has_memory_width: false,
            has_memory_copy: false,
            has_calldata_copy: false,
            has_storage: false,
            has_repeated_storage_slot: false,
            has_transient_storage: false,
            has_external_code: false,
            has_call: false,
            has_delegatecall: false,
            has_returndata: false,
            has_create: false,
            has_create2: false,
            has_log: false,
            max_log_topics: 0,
            has_branch: false,
            has_loop: false,
            has_return_exit: false,
            has_revert_exit: false,
            has_conditional_exit: false,
            has_stop_exit: false,
            has_invalid_exit: false,
            exit_kind: SeedExitKind::Stop,
            has_struct: false,
            has_tuple: false,
            has_compound_literal: false,
            has_field_access: false,
            has_field_update: false,
            has_comptime_type_reflection: false,
            has_cbytes_builtin: false,
            has_high_level_operator: false,
            has_core_ops_operator: false,
            has_helper_function: false,
            has_nested_helper_call: false,
            has_import: false,
            has_import_single: false,
            has_import_group: false,
            has_import_alias: false,
            has_import_glob: false,
            has_deep_import: false,
            has_comments: false,
            has_binary_literal: false,
            has_hex_literal: false,
            has_parameterized_type: false,
            has_comptime_control_flow: false,
            has_comptime_loop: false,
            has_type_dependent_branch: false,
            has_function_returns_compound: false,
            has_function_early_return: false,
            has_nested_compound: false,
            has_runtime_uninit: false,
            has_data_offset: false,
            has_generic_inferred_param: false,
            has_comptime_value_param: false,
            has_comptime_compound_param: false,
            has_function_valued_comptime: false,
            has_cbytes_edge_shapes: false,
            has_data_offset_dedup: false,
            has_data_offset_concat: false,
            has_anonymous_type_identity: false,
            has_cross_file_type_identity: false,
            has_eval_branch_quota: false,
            has_empty_compound: false,
            has_comptime_only_compound: false,
            has_runtime_only_compound: false,
            has_std_registered: false,
        }
    }
}
