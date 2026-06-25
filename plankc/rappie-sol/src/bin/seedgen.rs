use alloy_primitives as _;
use arbitrary::{Arbitrary, Unstructured};
use plank_driver as _;
use plank_evm as _;
use plank_source as _;
use rappie_sol::{
    FuzzCase, SeedCallKind, SeedClassification, SeedDynamicLenBucket, SeedEntryPosition,
    SeedExitKind, SeedProgramMode, execute_plank_solidity,
};
use revm as _;
use serde_json as _;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env, fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};

const DEFAULT_TARGET: usize = 48;
const DEFAULT_CANDIDATE_LIMIT: usize = 100_000;
const DEFAULT_OUTPUT_DIR: &str = "fuzz/seeds/plank_sol_program_diff";
const SIZES: [usize; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

const ALL_BUCKETS: &[Bucket] = &[
    Bucket::RawFallback,
    Bucket::SelectorDispatch1,
    Bucket::SelectorDispatch4,
    Bucket::SelectedFirstEntry,
    Bucket::SelectedLastEntry,
    Bucket::ReturnWords,
    Bucket::ReturnBytes,
    Bucket::RevertWords,
    Bucket::RevertBytes,
    Bucket::ConditionalReturn,
    Bucket::ConditionalRevert,
    Bucket::DynamicLenZero,
    Bucket::DynamicLenOne,
    Bucket::DynamicLenWordMinusOne,
    Bucket::DynamicLenWord,
    Bucket::DynamicLenWordPlusOne,
    Bucket::DynamicLenMax,
    Bucket::Log0,
    Bucket::Log4,
    Bucket::LogDataZero,
    Bucket::LogDataMax,
    Bucket::StorageOne,
    Bucket::StorageMax,
    Bucket::LoopZero,
    Bucket::LoopMax,
    Bucket::MemoryOne,
    Bucket::MemoryMax,
    Bucket::EchoCall,
    Bucket::EchoStaticCall,
    Bucket::RevertCall,
    Bucket::CalldataZero,
    Bucket::CalldataOne,
    Bucket::CalldataU64Max,
    Bucket::CalldataAlternating,
    Bucket::ShiftZero,
    Bucket::ShiftMax,
    Bucket::ByteIndexZero,
    Bucket::ByteIndexLast,
    Bucket::StressDispatchLogs,
    Bucket::StressDispatchStorage,
    Bucket::StressLoopCall,
    Bucket::StressDynamicConditional,
    Bucket::StressStorageLogsCall,
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
        let static_buckets = buckets_for(&classification, None);
        let combo = ComboKey::from(classification);
        let could_cover_conditional = classification.exit_kind == SeedExitKind::Conditional
            && (!covered.contains(&Bucket::ConditionalReturn)
                || !covered.contains(&Bucket::ConditionalRevert));
        let has_new_static_bucket = static_buckets.iter().any(|bucket| !covered.contains(bucket));
        let has_new_combo = !seen_combos.contains(&combo);

        if !has_new_static_bucket
            && !could_cover_conditional
            && (selected.len() >= config.target || !has_new_combo)
        {
            continue;
        }

        let success = if config.verify {
            match verified_success(&case) {
                Some(success) => success,
                None => {
                    stats.rejected += 1;
                    continue;
                }
            }
        } else {
            true
        };
        stats.verified += 1;

        let buckets = buckets_for(&classification, Some(success));
        let new_buckets =
            buckets.iter().copied().filter(|bucket| !covered.contains(bucket)).collect::<Vec<_>>();
        let has_new_combo = seen_combos.insert(combo);

        if new_buckets.is_empty() && (selected.len() >= config.target || !has_new_combo) {
            continue;
        }

        let name_bucket = new_buckets.first().copied().unwrap_or(Bucket::ComboExtra);
        let name = next_seed_name(name_bucket, &mut name_counts);
        covered.extend(buckets.iter().copied());
        selected.push(SelectedSeed { name, bytes, classification, buckets, success });

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
            "{}: {:?}, success={}, mode={:?}, exit={:?}, call={:?}",
            seed.name,
            seed.buckets,
            seed.success,
            seed.classification.mode,
            seed.classification.exit_kind,
            seed.classification.call_kind
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
    success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ComboKey {
    mode: SeedProgramMode,
    entry_count: usize,
    exit_kind: SeedExitKind,
    call_kind: SeedCallKind,
    log_topics: usize,
    storage_slots: usize,
    loop_iterations: usize,
    dynamic_len_bucket: SeedDynamicLenBucket,
}

impl From<SeedClassification> for ComboKey {
    fn from(classification: SeedClassification) -> Self {
        Self {
            mode: classification.mode,
            entry_count: classification.entry_count,
            exit_kind: classification.exit_kind,
            call_kind: classification.call_kind,
            log_topics: classification.log_topics,
            storage_slots: classification.storage_slots,
            loop_iterations: classification.loop_iterations,
            dynamic_len_bucket: classification.dynamic_len_bucket,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Bucket {
    RawFallback,
    SelectorDispatch1,
    SelectorDispatch4,
    SelectedFirstEntry,
    SelectedLastEntry,
    ReturnWords,
    ReturnBytes,
    RevertWords,
    RevertBytes,
    ConditionalReturn,
    ConditionalRevert,
    DynamicLenZero,
    DynamicLenOne,
    DynamicLenWordMinusOne,
    DynamicLenWord,
    DynamicLenWordPlusOne,
    DynamicLenMax,
    Log0,
    Log4,
    LogDataZero,
    LogDataMax,
    StorageOne,
    StorageMax,
    LoopZero,
    LoopMax,
    MemoryOne,
    MemoryMax,
    EchoCall,
    EchoStaticCall,
    RevertCall,
    CalldataZero,
    CalldataOne,
    CalldataU64Max,
    CalldataAlternating,
    ShiftZero,
    ShiftMax,
    ByteIndexZero,
    ByteIndexLast,
    StressDispatchLogs,
    StressDispatchStorage,
    StressLoopCall,
    StressDynamicConditional,
    StressStorageLogsCall,
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

fn verified_success(case: &FuzzCase) -> Option<bool> {
    let (plank, solidity) = execute_plank_solidity(case).ok()?;
    (plank.result == solidity.result).then_some(plank.result.success)
}

fn buckets_for(classification: &SeedClassification, success: Option<bool>) -> Vec<Bucket> {
    let mut buckets = Vec::new();

    match classification.mode {
        SeedProgramMode::RawFallback => buckets.push(Bucket::RawFallback),
        SeedProgramMode::SelectorDispatch => match classification.entry_count {
            1 => buckets.push(Bucket::SelectorDispatch1),
            4 => buckets.push(Bucket::SelectorDispatch4),
            _ => {}
        },
    }

    match classification.selected_entry_position {
        SeedEntryPosition::First => buckets.push(Bucket::SelectedFirstEntry),
        SeedEntryPosition::Last => buckets.push(Bucket::SelectedLastEntry),
        SeedEntryPosition::Only | SeedEntryPosition::Middle => {}
    }

    match classification.exit_kind {
        SeedExitKind::ReturnWords => buckets.push(Bucket::ReturnWords),
        SeedExitKind::ReturnBytes => buckets.push(Bucket::ReturnBytes),
        SeedExitKind::RevertWords => buckets.push(Bucket::RevertWords),
        SeedExitKind::RevertBytes => buckets.push(Bucket::RevertBytes),
        SeedExitKind::Conditional => match success {
            Some(true) => buckets.push(Bucket::ConditionalReturn),
            Some(false) => buckets.push(Bucket::ConditionalRevert),
            None => {}
        },
    }

    match classification.dynamic_len_bucket {
        SeedDynamicLenBucket::Zero => buckets.push(Bucket::DynamicLenZero),
        SeedDynamicLenBucket::One => buckets.push(Bucket::DynamicLenOne),
        SeedDynamicLenBucket::WordMinusOne => buckets.push(Bucket::DynamicLenWordMinusOne),
        SeedDynamicLenBucket::Word => buckets.push(Bucket::DynamicLenWord),
        SeedDynamicLenBucket::WordPlusOne => buckets.push(Bucket::DynamicLenWordPlusOne),
        SeedDynamicLenBucket::Max => buckets.push(Bucket::DynamicLenMax),
        SeedDynamicLenBucket::Other => {}
    }

    if classification.log_topics == 0 {
        buckets.push(Bucket::Log0);
    }
    if classification.log_topics == 4 {
        buckets.push(Bucket::Log4);
    }
    if classification.log_words == 0 {
        buckets.push(Bucket::LogDataZero);
    }
    if classification.log_words == 4 {
        buckets.push(Bucket::LogDataMax);
    }
    if classification.storage_slots == 1 {
        buckets.push(Bucket::StorageOne);
    }
    if classification.storage_slots == 3 {
        buckets.push(Bucket::StorageMax);
    }
    if classification.loop_iterations == 0 {
        buckets.push(Bucket::LoopZero);
    }
    if classification.loop_iterations == 8 {
        buckets.push(Bucket::LoopMax);
    }
    if classification.memory_slots == 1 {
        buckets.push(Bucket::MemoryOne);
    }
    if classification.memory_slots == 6 {
        buckets.push(Bucket::MemoryMax);
    }

    match classification.call_kind {
        SeedCallKind::EchoCall => buckets.push(Bucket::EchoCall),
        SeedCallKind::EchoStaticCall => buckets.push(Bucket::EchoStaticCall),
        SeedCallKind::RevertCall => buckets.push(Bucket::RevertCall),
    }

    if classification.has_zero_calldata {
        buckets.push(Bucket::CalldataZero);
    }
    if classification.has_one_calldata {
        buckets.push(Bucket::CalldataOne);
    }
    if classification.has_u64_max_calldata {
        buckets.push(Bucket::CalldataU64Max);
    }
    if classification.has_alternating_calldata {
        buckets.push(Bucket::CalldataAlternating);
    }
    if classification.shift == 0 {
        buckets.push(Bucket::ShiftZero);
    }
    if classification.shift == 255 {
        buckets.push(Bucket::ShiftMax);
    }
    if classification.byte_index == 0 {
        buckets.push(Bucket::ByteIndexZero);
    }
    if classification.byte_index == 31 {
        buckets.push(Bucket::ByteIndexLast);
    }

    if classification.mode == SeedProgramMode::SelectorDispatch
        && classification.entry_count == 4
        && classification.log_topics == 4
    {
        buckets.push(Bucket::StressDispatchLogs);
    }
    if classification.mode == SeedProgramMode::SelectorDispatch
        && classification.entry_count == 4
        && classification.storage_slots == 3
    {
        buckets.push(Bucket::StressDispatchStorage);
    }
    if classification.loop_iterations == 8 {
        buckets.push(Bucket::StressLoopCall);
    }
    if classification.exit_kind == SeedExitKind::Conditional
        && classification.dynamic_len_bucket == SeedDynamicLenBucket::Max
    {
        buckets.push(Bucket::StressDynamicConditional);
    }
    if classification.storage_slots == 3 && classification.log_topics >= 2 {
        buckets.push(Bucket::StressStorageLogsCall);
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
        Bucket::SelectorDispatch1 => "dispatch1",
        Bucket::SelectorDispatch4 => "dispatch4",
        Bucket::SelectedFirstEntry => "selected_first",
        Bucket::SelectedLastEntry => "selected_last",
        Bucket::ReturnWords => "return_words",
        Bucket::ReturnBytes => "return_bytes",
        Bucket::RevertWords => "revert_words",
        Bucket::RevertBytes => "revert_bytes",
        Bucket::ConditionalReturn => "conditional_return",
        Bucket::ConditionalRevert => "conditional_revert",
        Bucket::DynamicLenZero => "dynamic_zero",
        Bucket::DynamicLenOne => "dynamic_one",
        Bucket::DynamicLenWordMinusOne => "dynamic_31",
        Bucket::DynamicLenWord => "dynamic_32",
        Bucket::DynamicLenWordPlusOne => "dynamic_33",
        Bucket::DynamicLenMax => "dynamic_max",
        Bucket::Log0 => "log0",
        Bucket::Log4 => "log4",
        Bucket::LogDataZero => "log_data_zero",
        Bucket::LogDataMax => "log_data_max",
        Bucket::StorageOne => "storage_one",
        Bucket::StorageMax => "storage_max",
        Bucket::LoopZero => "loop_zero",
        Bucket::LoopMax => "loop_max",
        Bucket::MemoryOne => "memory_one",
        Bucket::MemoryMax => "memory_max",
        Bucket::EchoCall => "call_echo",
        Bucket::EchoStaticCall => "call_static_echo",
        Bucket::RevertCall => "call_revert",
        Bucket::CalldataZero => "calldata_zero",
        Bucket::CalldataOne => "calldata_one",
        Bucket::CalldataU64Max => "calldata_u64_max",
        Bucket::CalldataAlternating => "calldata_alternating",
        Bucket::ShiftZero => "shift_zero",
        Bucket::ShiftMax => "shift_max",
        Bucket::ByteIndexZero => "byte_zero",
        Bucket::ByteIndexLast => "byte_last",
        Bucket::StressDispatchLogs => "stress_dispatch_logs",
        Bucket::StressDispatchStorage => "stress_dispatch_storage",
        Bucket::StressLoopCall => "stress_loop_call",
        Bucket::StressDynamicConditional => "stress_dynamic_conditional",
        Bucket::StressStorageLogsCall => "stress_storage_logs_call",
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
        candidates.push(repeated_pattern(size, b"RappieSolSeed"));
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
    match rng.next_u64() % 4 {
        0 => random_bytes(size, &mut rng),
        1 => word_pattern_bytes(size, &mut rng),
        2 => sparse_edge_bytes(size, &mut rng),
        _ => mixed_pattern_bytes(size, &mut rng),
    }
}

fn random_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    (0..size).map(|_| rng.next_u64() as u8).collect()
}

fn word_pattern_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    const WORDS: [u64; 6] =
        [0, 1, u64::MAX, 0x5555_5555_5555_5555, 0xaaaa_aaaa_aaaa_aaaa, 0x8000_0000_0000_0000];

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
        let roll = rng.next_u64() % 16;
        *byte = match roll {
            0 => 0,
            1 => 1,
            2 => 31,
            3 => 32,
            4 => 33,
            5 => 160,
            6 => 255,
            _ => rng.next_u64() as u8,
        };
    }
    bytes
}

fn mixed_pattern_bytes(size: usize, rng: &mut XorShift64) -> Vec<u8> {
    let pattern = match rng.next_u64() % 3 {
        0 => b"SelectorDispatchLogsStorageCalls".as_slice(),
        1 => b"ReturnRevertDynamicLoopMemory".as_slice(),
        _ => b"\x00\x01\x1f\x20\x21\xa5\x5a\xff".as_slice(),
    };
    repeated_pattern(size, pattern)
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
