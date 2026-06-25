use crate::evm::{
    CALLER_WORD, EMPTY_ACCOUNT_WORD, HELPER_CODE_WORD, HELPER_ECHO_WORD, HELPER_REVERT_WORD,
};
use arbitrary::{Arbitrary, Unstructured};
use std::{collections::BTreeSet, fmt::Write};

const MAX_DISPATCH_ENTRIES: usize = 6;
const MAX_CALL_STEPS: usize = 4;
const MAX_FRAGMENTS_PER_ENTRY: usize = 10;
const MAX_CALLDATA_PAYLOAD_BYTES: usize = 192;
const MAX_RETURN_BYTES: usize = 192;
const MAX_LOOP_ITERATIONS: usize = 8;
const SCRATCH_BYTES: usize = 2048;
const MAX_SAFE_OFFSET: usize = 1536;
const CALL_INPUT_OFFSET: usize = 512;
const CALL_OUTPUT_OFFSET: usize = 608;
const RETURN_DATA_OFFSET: usize = 704;
const EXTCODE_OFFSET: usize = 832;
const CREATE_OFFSET: usize = 960;
const LOOP_MEMORY_OFFSET: usize = 1088;

const U256_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const I256_MIN: &str = "0x8000000000000000000000000000000000000000000000000000000000000000";
const I256_MAX: &str = "0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const CREATE_INIT_BYTES: usize = 13;
const CREATE_INIT_WORD: &str = "0x6001600c60003960016000f30000000000000000000000000000000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SeedProgramMode {
    RawFallback,
    SelectorDispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SeedExitKind {
    Return,
    Revert,
    ConditionalReturnRevert,
    Stop,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeedClassification {
    pub mode: SeedProgramMode,
    pub entry_count: usize,
    pub call_count: usize,
    pub touches_multiple_entries: bool,
    pub has_short_calldata: bool,
    pub has_unaligned_calldata: bool,
    pub has_full_width_word: bool,
    pub has_arithmetic: bool,
    pub has_signed_arithmetic: bool,
    pub has_memory_width: bool,
    pub has_memory_copy: bool,
    pub has_calldata_copy: bool,
    pub has_storage: bool,
    pub has_repeated_storage_slot: bool,
    pub has_transient_storage: bool,
    pub has_external_code: bool,
    pub has_call: bool,
    pub has_delegatecall: bool,
    pub has_returndata: bool,
    pub has_create: bool,
    pub has_create2: bool,
    pub has_log: bool,
    pub max_log_topics: usize,
    pub has_branch: bool,
    pub has_loop: bool,
    pub has_return_exit: bool,
    pub has_revert_exit: bool,
    pub has_conditional_exit: bool,
    pub has_stop_exit: bool,
    pub has_invalid_exit: bool,
    pub exit_kind: SeedExitKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedCase {
    mode: ProgramMode,
    entries: Vec<Entry>,
    calls: Vec<CallStep>,
}

impl GeneratedCase {
    pub(crate) fn plank_source(&self) -> String {
        let mut source = String::new();

        if self.mode == ProgramMode::SelectorDispatch {
            for (index, entry) in self.entries.iter().enumerate() {
                writeln!(source, "const SELECTOR_{index} = {};", hex_u32(entry.selector))
                    .expect("writing to a string cannot fail");
            }
            source.push('\n');
        }

        for (index, entry) in self.entries.iter().enumerate() {
            render_plank_entry(&mut source, index, entry, self.mode);
            source.push('\n');
        }

        source.push_str("init {\n");
        match self.mode {
            ProgramMode::RawFallback => {
                source.push_str("    entry_0();\n");
            }
            ProgramMode::SelectorDispatch => {
                source.push_str("    let selector = @evm_shr(224, @evm_calldataload(0));\n");
                render_plank_dispatch(&mut source, self.entries.len(), 0, 1);
            }
        }
        source.push_str("}\n");

        source
    }

    pub(crate) fn solidity_source(&self) -> String {
        let mut source = String::new();
        source.push_str("// SPDX-License-Identifier: MIT\n");
        source.push_str("pragma solidity >=0.8.20;\n\n");
        source.push_str("contract C {\n");
        source.push_str("    fallback() external payable {\n");
        source.push_str("        assembly (\"memory-safe\") {\n");

        for (index, entry) in self.entries.iter().enumerate() {
            render_yul_entry(&mut source, index, entry, self.mode);
            source.push('\n');
        }

        match self.mode {
            ProgramMode::RawFallback => {
                source.push_str("            entry_0()\n");
            }
            ProgramMode::SelectorDispatch => {
                source.push_str("            let selector := shr(224, calldataload(0))\n");
                source.push_str("            switch selector\n");
                for (index, entry) in self.entries.iter().enumerate() {
                    writeln!(
                        source,
                        "            case {} {{ entry_{index}() }}",
                        hex_u32(entry.selector)
                    )
                    .expect("writing to a string cannot fail");
                }
                source.push_str("            default { revert(0, 0) }\n");
            }
        }

        source.push_str("        }\n");
        source.push_str("    }\n");
        source.push_str("}\n");

        source
    }

    pub(crate) fn calldatas(&self) -> Vec<Vec<u8>> {
        self.calls
            .iter()
            .map(|call| {
                let mut calldata = Vec::with_capacity(
                    call.payload.len()
                        + if self.mode == ProgramMode::SelectorDispatch { 4 } else { 0 },
                );
                if self.mode == ProgramMode::SelectorDispatch {
                    calldata.extend_from_slice(
                        &self.entries[call.selected_entry].selector.to_be_bytes(),
                    );
                }
                calldata.extend_from_slice(&call.payload);
                calldata
            })
            .collect()
    }

    pub(crate) fn call_count(&self) -> usize {
        self.calls.len()
    }

    pub(crate) fn seed_classification(&self) -> SeedClassification {
        let active_entries =
            self.calls.iter().map(|call| call.selected_entry).collect::<BTreeSet<_>>();
        let mut classification = SeedClassification {
            mode: self.mode.into(),
            entry_count: self.entries.len(),
            call_count: self.calls.len(),
            touches_multiple_entries: active_entries.len() > 1,
            has_short_calldata: self.calls.iter().any(|call| call.payload.len() < 32)
                || self.calls.iter().any(|call| call.payload.len() % 32 != 0),
            has_unaligned_calldata: false,
            has_full_width_word: self.calls.iter().any(|call| has_full_width_word(&call.payload)),
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
            exit_kind: self
                .calls
                .first()
                .map(|call| self.entries[call.selected_entry].config.exit.kind.into())
                .unwrap_or(SeedExitKind::Stop),
        };

        for entry_index in active_entries {
            let entry = &self.entries[entry_index];
            entry.config.exit.kind.classify(&mut classification);
            for fragment in &entry.config.fragments {
                fragment.classify(&mut classification);
            }
        }

        classification
    }
}

impl<'a> Arbitrary<'a> for GeneratedCase {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let mode = ProgramMode::arbitrary(u)?;
        let entry_count = match mode {
            ProgramMode::RawFallback => 1,
            ProgramMode::SelectorDispatch => u.int_in_range(1..=MAX_DISPATCH_ENTRIES)?,
        };
        let mut selectors = Vec::with_capacity(entry_count);
        let mut entries = Vec::with_capacity(entry_count);

        for index in 0..entry_count {
            let selector = unique_selector(u, index, &selectors)?;
            selectors.push(selector);
            entries.push(Entry::arbitrary(u, selector)?);
        }

        let call_count = u.int_in_range(1..=MAX_CALL_STEPS)?;
        let mut calls = Vec::with_capacity(call_count);
        for _ in 0..call_count {
            calls.push(CallStep::arbitrary(u, entry_count)?);
        }

        Ok(Self { mode, entries, calls })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramMode {
    RawFallback,
    SelectorDispatch,
}

impl From<ProgramMode> for SeedProgramMode {
    fn from(mode: ProgramMode) -> Self {
        match mode {
            ProgramMode::RawFallback => Self::RawFallback,
            ProgramMode::SelectorDispatch => Self::SelectorDispatch,
        }
    }
}

impl ProgramMode {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(if u.int_in_range(0..=3)? == 0 { Self::RawFallback } else { Self::SelectorDispatch })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    selector: u32,
    config: EntryConfig,
}

impl Entry {
    fn arbitrary(u: &mut Unstructured<'_>, selector: u32) -> arbitrary::Result<Self> {
        Ok(Self { selector, config: EntryConfig::arbitrary(u)? })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryConfig {
    fragments: Vec<Fragment>,
    exit: ExitConfig,
    constants: ConstantPool,
}

impl EntryConfig {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let constants = ConstantPool::arbitrary(u)?;
        let fragment_count = u.int_in_range(3..=MAX_FRAGMENTS_PER_ENTRY)?;
        let mut fragments = Vec::with_capacity(fragment_count);
        for _ in 0..fragment_count {
            fragments.push(Fragment::arbitrary(u)?);
        }
        let exit = ExitConfig::arbitrary(u)?;

        Ok(Self { fragments, exit, constants })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CallStep {
    selected_entry: usize,
    payload: Vec<u8>,
}

impl CallStep {
    fn arbitrary(u: &mut Unstructured<'_>, entry_count: usize) -> arbitrary::Result<Self> {
        let selected_entry = u.int_in_range(0..=entry_count - 1)?;
        let len = u.int_in_range(0..=MAX_CALLDATA_PAYLOAD_BYTES)?;
        let payload = (0..len).map(|_| u8::arbitrary(u)).collect::<arbitrary::Result<Vec<_>>>()?;

        Ok(Self { selected_entry, payload })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConstantPool {
    words: [[u8; 32]; 4],
}

impl ConstantPool {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(Self {
            words: [arbitrary_word(u)?, arbitrary_word(u)?, arbitrary_word(u)?, arbitrary_word(u)?],
        })
    }

    fn expr(&self, index: usize) -> String {
        match index % 14 {
            0 => hex_word(self.words[0]),
            1 => hex_word(self.words[1]),
            2 => hex_word(self.words[2]),
            3 => hex_word(self.words[3]),
            4 => "0x0".to_string(),
            5 => "0x1".to_string(),
            6 => "0xff".to_string(),
            7 => "0xffff".to_string(),
            8 => I256_MIN.to_string(),
            9 => I256_MAX.to_string(),
            10 => U256_MAX.to_string(),
            11 => "0x8000000000000000".to_string(),
            12 => "0x5555555555555555555555555555555555555555555555555555555555555555".to_string(),
            _ => "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExitConfig {
    kind: ExitKind,
    output_len: usize,
}

impl ExitConfig {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(Self {
            kind: ExitKind::arbitrary(u)?,
            output_len: u.int_in_range(0..=MAX_RETURN_BYTES)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    Return,
    Revert,
    ConditionalReturnRevert,
    Stop,
    Invalid,
}

impl From<ExitKind> for SeedExitKind {
    fn from(kind: ExitKind) -> Self {
        match kind {
            ExitKind::Return => Self::Return,
            ExitKind::Revert => Self::Revert,
            ExitKind::ConditionalReturnRevert => Self::ConditionalReturnRevert,
            ExitKind::Stop => Self::Stop,
            ExitKind::Invalid => Self::Invalid,
        }
    }
}

impl ExitKind {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=4)? {
            0 => Self::Return,
            1 => Self::Revert,
            2 => Self::ConditionalReturnRevert,
            3 => Self::Stop,
            _ => Self::Invalid,
        })
    }

    fn classify(self, classification: &mut SeedClassification) {
        match self {
            Self::Return => classification.has_return_exit = true,
            Self::Revert => classification.has_revert_exit = true,
            Self::ConditionalReturnRevert => classification.has_conditional_exit = true,
            Self::Stop => classification.has_stop_exit = true,
            Self::Invalid => classification.has_invalid_exit = true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Fragment {
    Calldata { op: CalldataOp, offset: usize, len: usize, dst: usize },
    Arithmetic { op: ArithmeticOp, lhs: usize, rhs: usize, aux: usize },
    Memory { width: usize, offset: usize, value: usize },
    MemoryCopy { dst: usize, src: usize, len: usize },
    Storage { slot: usize, repeated: bool },
    TransientStorage { slot: usize },
    Environment { op: EnvironmentOp },
    ExternalCode { op: ExternalCodeOp, target: AddressTarget, offset: usize, len: usize },
    Call { kind: CallKind, input_len: usize, output_len: usize, returndata_len: usize },
    Create { kind: CreateKind, salt: usize },
    Log { topics: usize, len: usize },
    Branch { left: usize, right: usize },
    Loop { iterations: usize, value: usize },
}

impl Fragment {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=11)? {
            0 => Self::Calldata {
                op: CalldataOp::arbitrary(u)?,
                offset: u.int_in_range(0..=96)?,
                len: u.int_in_range(0..=96)?,
                dst: u.int_in_range(0..=MAX_SAFE_OFFSET)?,
            },
            1 => Self::Arithmetic {
                op: ArithmeticOp::arbitrary(u)?,
                lhs: u.int_in_range(0..=13)?,
                rhs: u.int_in_range(0..=13)?,
                aux: u.int_in_range(0..=255)?,
            },
            2 => Self::Memory {
                width: u.int_in_range(1..=32)?,
                offset: u.int_in_range(0..=MAX_SAFE_OFFSET)?,
                value: u.int_in_range(0..=13)?,
            },
            3 => Self::MemoryCopy {
                dst: u.int_in_range(0..=MAX_SAFE_OFFSET)?,
                src: u.int_in_range(0..=MAX_SAFE_OFFSET)?,
                len: u.int_in_range(0..=96)?,
            },
            4 => Self::Storage { slot: u.int_in_range(0..=5)?, repeated: bool::arbitrary(u)? },
            5 => Self::TransientStorage { slot: u.int_in_range(0..=5)? },
            6 => Self::Environment { op: EnvironmentOp::arbitrary(u)? },
            7 => Self::ExternalCode {
                op: ExternalCodeOp::arbitrary(u)?,
                target: AddressTarget::arbitrary(u)?,
                offset: u.int_in_range(0..=16)?,
                len: u.int_in_range(0..=64)?,
            },
            8 => Self::Call {
                kind: CallKind::arbitrary(u)?,
                input_len: u.int_in_range(0..=64)?,
                output_len: u.int_in_range(0..=64)?,
                returndata_len: u.int_in_range(0..=32)?,
            },
            9 => Self::Create { kind: CreateKind::arbitrary(u)?, salt: u.int_in_range(0..=13)? },
            10 => Self::Log { topics: u.int_in_range(0..=4)?, len: u.int_in_range(0..=128)? },
            _ => {
                if bool::arbitrary(u)? {
                    Self::Branch { left: u.int_in_range(0..=13)?, right: u.int_in_range(0..=13)? }
                } else {
                    Self::Loop {
                        iterations: u.int_in_range(0..=MAX_LOOP_ITERATIONS)?,
                        value: u.int_in_range(0..=13)?,
                    }
                }
            }
        })
    }

    fn classify(&self, classification: &mut SeedClassification) {
        match self {
            Self::Calldata { op, offset, .. } => {
                classification.has_calldata_copy |= *op == CalldataOp::Copy;
                classification.has_unaligned_calldata |= *offset % 32 != 0;
            }
            Self::Arithmetic { op, .. } => {
                classification.has_arithmetic = true;
                classification.has_signed_arithmetic |= op.is_signed();
            }
            Self::Memory { width, .. } => {
                classification.has_memory_width |= *width != 32;
            }
            Self::MemoryCopy { .. } => classification.has_memory_copy = true,
            Self::Storage { repeated, .. } => {
                classification.has_storage = true;
                classification.has_repeated_storage_slot |= *repeated;
            }
            Self::TransientStorage { .. } => classification.has_transient_storage = true,
            Self::Environment { .. } => {}
            Self::ExternalCode { .. } => classification.has_external_code = true,
            Self::Call { kind, returndata_len, .. } => {
                classification.has_call = true;
                classification.has_delegatecall |= kind.is_delegatecall();
                classification.has_returndata |= *returndata_len > 0;
            }
            Self::Create { kind, .. } => match kind {
                CreateKind::Create => classification.has_create = true,
                CreateKind::Create2 => classification.has_create2 = true,
            },
            Self::Log { topics, .. } => {
                classification.has_log = true;
                classification.max_log_topics = classification.max_log_topics.max(*topics);
            }
            Self::Branch { .. } => classification.has_branch = true,
            Self::Loop { iterations, .. } => {
                classification.has_loop = true;
                classification.has_loop |= *iterations == MAX_LOOP_ITERATIONS;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalldataOp {
    Size,
    Load,
    Copy,
}

impl CalldataOp {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=2)? {
            0 => Self::Size,
            1 => Self::Load,
            _ => Self::Copy,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
    SDiv,
    Mod,
    SMod,
    AddMod,
    MulMod,
    Exp,
    SignExtend,
    Lt,
    Gt,
    SLt,
    SGt,
    Eq,
    IsZero,
    And,
    Or,
    Xor,
    Not,
    Byte,
    Shl,
    Shr,
    Sar,
}

impl ArithmeticOp {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=24)? {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::Div,
            4 => Self::SDiv,
            5 => Self::Mod,
            6 => Self::SMod,
            7 => Self::AddMod,
            8 => Self::MulMod,
            9 => Self::Exp,
            10 => Self::SignExtend,
            11 => Self::Lt,
            12 => Self::Gt,
            13 => Self::SLt,
            14 => Self::SGt,
            15 => Self::Eq,
            16 => Self::IsZero,
            17 => Self::And,
            18 => Self::Or,
            19 => Self::Xor,
            20 => Self::Not,
            21 => Self::Byte,
            22 => Self::Shl,
            23 => Self::Shr,
            _ => Self::Sar,
        })
    }

    fn is_signed(self) -> bool {
        matches!(
            self,
            Self::SDiv | Self::SMod | Self::SignExtend | Self::SLt | Self::SGt | Self::Sar
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvironmentOp {
    Address,
    Balance,
    Origin,
    Caller,
    CallValue,
    GasPrice,
    BlockHash,
    Coinbase,
    Timestamp,
    Number,
    Difficulty,
    GasLimit,
    ChainId,
    SelfBalance,
    BaseFee,
}

impl EnvironmentOp {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=14)? {
            0 => Self::Address,
            1 => Self::Balance,
            2 => Self::Origin,
            3 => Self::Caller,
            4 => Self::CallValue,
            5 => Self::GasPrice,
            6 => Self::BlockHash,
            7 => Self::Coinbase,
            8 => Self::Timestamp,
            9 => Self::Number,
            10 => Self::Difficulty,
            11 => Self::GasLimit,
            12 => Self::ChainId,
            13 => Self::SelfBalance,
            _ => Self::BaseFee,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalCodeOp {
    Size,
    Hash,
    Copy,
}

impl ExternalCodeOp {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=2)? {
            0 => Self::Size,
            1 => Self::Hash,
            _ => Self::Copy,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressTarget {
    Caller,
    Echo,
    Revert,
    Code,
    Empty,
}

impl AddressTarget {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=4)? {
            0 => Self::Caller,
            1 => Self::Echo,
            2 => Self::Revert,
            3 => Self::Code,
            _ => Self::Empty,
        })
    }

    fn expr(self) -> &'static str {
        match self {
            Self::Caller => CALLER_WORD,
            Self::Echo => HELPER_ECHO_WORD,
            Self::Revert => HELPER_REVERT_WORD,
            Self::Code => HELPER_CODE_WORD,
            Self::Empty => EMPTY_ACCOUNT_WORD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    CallEcho,
    StaticEcho,
    Revert,
    DelegateEcho,
}

impl CallKind {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=3)? {
            0 => Self::CallEcho,
            1 => Self::StaticEcho,
            2 => Self::Revert,
            _ => Self::DelegateEcho,
        })
    }

    fn is_delegatecall(self) -> bool {
        matches!(self, Self::DelegateEcho)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateKind {
    Create,
    Create2,
}

impl CreateKind {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(if bool::arbitrary(u)? { Self::Create } else { Self::Create2 })
    }
}

fn render_plank_dispatch(
    source: &mut String,
    entry_count: usize,
    entry_index: usize,
    indent_depth: usize,
) {
    let indent = "    ".repeat(indent_depth);
    if entry_index >= entry_count {
        writeln!(source, "{indent}@evm_revert(@malloc_uninit(0), 0);")
            .expect("writing to a string cannot fail");
        return;
    }

    writeln!(source, "{indent}if @evm_eq(selector, SELECTOR_{entry_index}) {{")
        .expect("writing to a string cannot fail");
    writeln!(source, "{indent}    entry_{entry_index}();")
        .expect("writing to a string cannot fail");
    writeln!(source, "{indent}}} else {{").expect("writing to a string cannot fail");
    render_plank_dispatch(source, entry_count, entry_index + 1, indent_depth + 1);
    writeln!(source, "{indent}}}").expect("writing to a string cannot fail");
}

fn render_plank_entry(source: &mut String, entry_index: usize, entry: &Entry, mode: ProgramMode) {
    writeln!(source, "const entry_{entry_index} = fn () never {{")
        .expect("writing to a string cannot fail");
    writeln!(source, "    let scratch = @malloc_zeroed({SCRATCH_BYTES});")
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "    let mut acc = @evm_xor(@evm_calldataload({}), {});",
        calldata_offset(mode, 0),
        entry.config.constants.expr(0)
    )
    .expect("writing to a string cannot fail");

    for (fragment_index, fragment) in entry.config.fragments.iter().enumerate() {
        render_plank_fragment(source, entry_index, fragment_index, fragment, &entry.config);
    }

    render_plank_exit(source, &entry.config);
    source.push_str("};\n");
}

fn render_plank_fragment(
    source: &mut String,
    entry_index: usize,
    fragment_index: usize,
    fragment: &Fragment,
    cfg: &EntryConfig,
) {
    match fragment {
        Fragment::Calldata { op, offset, len, dst } => match op {
            CalldataOp::Size => source.push_str("    acc = @evm_xor(acc, @evm_calldatasize());\n"),
            CalldataOp::Load => {
                writeln!(source, "    acc = @evm_xor(acc, @evm_calldataload({offset}));")
                    .expect("writing to a string cannot fail");
            }
            CalldataOp::Copy => {
                writeln!(
                    source,
                    "    @evm_calldatacopy({}, {offset}, {len});",
                    plank_ptr("scratch", *dst)
                )
                .expect("writing to a string cannot fail");
                writeln!(
                    source,
                    "    acc = @evm_xor(acc, @evm_keccak256({}, {len}));",
                    plank_ptr("scratch", *dst)
                )
                .expect("writing to a string cannot fail");
            }
        },
        Fragment::Arithmetic { op, lhs, rhs, aux } => {
            render_plank_arithmetic(source, *op, cfg, *lhs, *rhs, *aux);
        }
        Fragment::Memory { width, offset, value } => {
            let ptr = plank_ptr("scratch", *offset);
            writeln!(
                source,
                "    @mstore{width}({ptr}, @evm_add(acc, {}));",
                cfg.constants.expr(*value)
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, @mload{width}({ptr}));")
                .expect("writing to a string cannot fail");
        }
        Fragment::MemoryCopy { dst, src, len } => {
            writeln!(
                source,
                "    @mcopy({}, {}, {len});",
                plank_ptr("scratch", *dst),
                plank_ptr("scratch", *src)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, @evm_keccak256({}, {len}));",
                plank_ptr("scratch", *dst)
            )
            .expect("writing to a string cannot fail");
        }
        Fragment::Storage { slot, repeated } => {
            let storage_slot = storage_slot(entry_index, *slot, cfg);
            writeln!(source, "    acc = @evm_xor(acc, @evm_sload({}));", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    @evm_sstore({}, @evm_xor(acc, {}));",
                hex_u64(storage_slot),
                cfg.constants.expr(*slot)
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_add(acc, @evm_sload({}));", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
            if *repeated {
                writeln!(
                    source,
                    "    @evm_sstore({}, @evm_xor(acc, {}));",
                    hex_u64(storage_slot),
                    cfg.constants.expr(*slot + 1)
                )
                .expect("writing to a string cannot fail");
                writeln!(source, "    acc = @evm_xor(acc, @evm_sload({}));", hex_u64(storage_slot))
                    .expect("writing to a string cannot fail");
            }
        }
        Fragment::TransientStorage { slot } => {
            let storage_slot = storage_slot(entry_index, *slot, cfg);
            writeln!(
                source,
                "    @evm_tstore({}, @evm_xor(acc, {}));",
                hex_u64(storage_slot),
                cfg.constants.expr(*slot)
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, @evm_tload({}));", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
        }
        Fragment::Environment { op } => render_plank_environment(source, *op),
        Fragment::ExternalCode { op, target, offset, len } => {
            render_plank_external_code(source, *op, *target, *offset, *len);
        }
        Fragment::Call { kind, input_len, output_len, returndata_len } => {
            render_plank_call(source, *kind, *input_len, *output_len, *returndata_len);
        }
        Fragment::Create { kind, salt } => {
            let ptr = plank_ptr("scratch", CREATE_OFFSET);
            writeln!(source, "    @mstore32({ptr}, {CREATE_INIT_WORD});")
                .expect("writing to a string cannot fail");
            match kind {
                CreateKind::Create => {
                    writeln!(
                        source,
                        "    let created_{fragment_index} = @evm_create(0, {ptr}, {CREATE_INIT_BYTES});"
                    )
                    .expect("writing to a string cannot fail");
                }
                CreateKind::Create2 => {
                    writeln!(
                        source,
                        "    let created_{fragment_index} = @evm_create2(0, {ptr}, {CREATE_INIT_BYTES}, {});",
                        cfg.constants.expr(*salt)
                    )
                    .expect("writing to a string cannot fail");
                }
            }
            writeln!(source, "    acc = @evm_xor(acc, created_{fragment_index});")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, @evm_extcodesize(created_{fragment_index}));"
            )
            .expect("writing to a string cannot fail");
        }
        Fragment::Log { topics, len } => {
            for word in 0..words_for_len(*len) {
                writeln!(
                    source,
                    "    @mstore32({}, @evm_add(acc, {}));",
                    plank_ptr("scratch", word * 32),
                    cfg.constants.expr(word)
                )
                .expect("writing to a string cannot fail");
            }
            let mut args = vec!["scratch".to_string(), len.to_string()];
            args.extend((0..*topics).map(|topic| plank_topic_expr(cfg, topic)));
            writeln!(source, "    @evm_log{}({});", topics, args.join(", "))
                .expect("writing to a string cannot fail");
        }
        Fragment::Branch { left, right } => {
            source.push_str("    if @evm_iszero(@evm_and(acc, 1)) {\n");
            writeln!(source, "        acc = @evm_add(acc, {});", cfg.constants.expr(*left))
                .expect("writing to a string cannot fail");
            source.push_str("    } else {\n");
            writeln!(source, "        acc = @evm_xor(acc, {});", cfg.constants.expr(*right))
                .expect("writing to a string cannot fail");
            source.push_str("    }\n");
        }
        Fragment::Loop { iterations, value } => {
            source.push_str("    let mut i = 0;\n");
            writeln!(source, "    while @evm_lt(i, {iterations}) {{")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "        acc = @evm_add(acc, @evm_xor(i, {}));",
                cfg.constants.expr(*value)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "        @mstore32({}, acc);",
                plank_ptr("scratch", LOOP_MEMORY_OFFSET)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "        acc = @evm_xor(acc, @mload32({}));",
                plank_ptr("scratch", LOOP_MEMORY_OFFSET)
            )
            .expect("writing to a string cannot fail");
            source.push_str("        i = @evm_add(i, 1);\n");
            source.push_str("    }\n");
        }
    }
}

fn render_plank_arithmetic(
    source: &mut String,
    op: ArithmeticOp,
    cfg: &EntryConfig,
    lhs: usize,
    rhs: usize,
    aux: usize,
) {
    let left = cfg.constants.expr(lhs);
    let right = cfg.constants.expr(rhs);
    match op {
        ArithmeticOp::Add => writeln!(source, "    acc = @evm_add(acc, {right});"),
        ArithmeticOp::Sub => writeln!(source, "    acc = @evm_sub(acc, {right});"),
        ArithmeticOp::Mul => writeln!(source, "    acc = @evm_mul(acc, {right});"),
        ArithmeticOp::Div => writeln!(source, "    acc = @evm_div(acc, {right});"),
        ArithmeticOp::SDiv => writeln!(source, "    acc = @evm_sdiv(acc, {right});"),
        ArithmeticOp::Mod => writeln!(source, "    acc = @evm_mod(acc, {right});"),
        ArithmeticOp::SMod => writeln!(source, "    acc = @evm_smod(acc, {right});"),
        ArithmeticOp::AddMod => writeln!(source, "    acc = @evm_addmod(acc, {left}, {right});"),
        ArithmeticOp::MulMod => writeln!(source, "    acc = @evm_mulmod(acc, {left}, {right});"),
        ArithmeticOp::Exp => writeln!(source, "    acc = @evm_exp(acc, @evm_and({right}, 0xff));"),
        ArithmeticOp::SignExtend => {
            writeln!(source, "    acc = @evm_signextend({}, acc);", aux % 32)
        }
        ArithmeticOp::Lt => render_plank_bool_xor(source, &format!("@evm_lt(acc, {right})")),
        ArithmeticOp::Gt => render_plank_bool_xor(source, &format!("@evm_gt(acc, {right})")),
        ArithmeticOp::SLt => render_plank_bool_xor(source, &format!("@evm_slt(acc, {right})")),
        ArithmeticOp::SGt => render_plank_bool_xor(source, &format!("@evm_sgt(acc, {right})")),
        ArithmeticOp::Eq => render_plank_bool_xor(source, &format!("@evm_eq(acc, {right})")),
        ArithmeticOp::IsZero => render_plank_bool_xor(source, "@evm_iszero(acc)"),
        ArithmeticOp::And => writeln!(source, "    acc = @evm_and(acc, {right});"),
        ArithmeticOp::Or => writeln!(source, "    acc = @evm_or(acc, {right});"),
        ArithmeticOp::Xor => writeln!(source, "    acc = @evm_xor(acc, {right});"),
        ArithmeticOp::Not => writeln!(source, "    acc = @evm_not(acc);"),
        ArithmeticOp::Byte => {
            writeln!(source, "    acc = @evm_xor(acc, @evm_byte({}, {right}));", aux % 32)
        }
        ArithmeticOp::Shl => writeln!(source, "    acc = @evm_shl({}, acc);", aux),
        ArithmeticOp::Shr => writeln!(source, "    acc = @evm_shr({}, acc);", aux),
        ArithmeticOp::Sar => writeln!(source, "    acc = @evm_sar({}, acc);", aux),
    }
    .expect("writing to a string cannot fail");
}

fn render_plank_bool_xor(source: &mut String, condition: &str) -> std::fmt::Result {
    writeln!(source, "    if {condition} {{")?;
    writeln!(source, "        acc = @evm_xor(acc, 1);")?;
    writeln!(source, "    }}")
}

fn render_plank_environment(source: &mut String, op: EnvironmentOp) {
    match op {
        EnvironmentOp::Address => {
            source.push_str("    acc = @evm_xor(acc, @evm_address_this());\n")
        }
        EnvironmentOp::Balance => {
            writeln!(source, "    acc = @evm_xor(acc, @evm_balance({HELPER_ECHO_WORD}));")
                .expect("writing to a string cannot fail");
        }
        EnvironmentOp::Origin => source.push_str("    acc = @evm_xor(acc, @evm_origin());\n"),
        EnvironmentOp::Caller => source.push_str("    acc = @evm_xor(acc, @evm_caller());\n"),
        EnvironmentOp::CallValue => source.push_str("    acc = @evm_xor(acc, @evm_callvalue());\n"),
        EnvironmentOp::GasPrice => source.push_str("    acc = @evm_xor(acc, @evm_gasprice());\n"),
        EnvironmentOp::BlockHash => {
            source
                .push_str("    acc = @evm_xor(acc, @evm_blockhash(@evm_sub(@evm_number(), 1)));\n");
        }
        EnvironmentOp::Coinbase => source.push_str("    acc = @evm_xor(acc, @evm_coinbase());\n"),
        EnvironmentOp::Timestamp => source.push_str("    acc = @evm_xor(acc, @evm_timestamp());\n"),
        EnvironmentOp::Number => source.push_str("    acc = @evm_xor(acc, @evm_number());\n"),
        EnvironmentOp::Difficulty => {
            source.push_str("    acc = @evm_xor(acc, @evm_difficulty());\n");
        }
        EnvironmentOp::GasLimit => source.push_str("    acc = @evm_xor(acc, @evm_gaslimit());\n"),
        EnvironmentOp::ChainId => source.push_str("    acc = @evm_xor(acc, @evm_chainid());\n"),
        EnvironmentOp::SelfBalance => {
            source.push_str("    acc = @evm_xor(acc, @evm_selfbalance());\n");
        }
        EnvironmentOp::BaseFee => source.push_str("    acc = @evm_xor(acc, @evm_basefee());\n"),
    }
}

fn render_plank_external_code(
    source: &mut String,
    op: ExternalCodeOp,
    target: AddressTarget,
    offset: usize,
    len: usize,
) {
    match op {
        ExternalCodeOp::Size => {
            writeln!(source, "    acc = @evm_xor(acc, @evm_extcodesize({}));", target.expr())
                .expect("writing to a string cannot fail");
        }
        ExternalCodeOp::Hash => {
            writeln!(source, "    acc = @evm_xor(acc, @evm_extcodehash({}));", target.expr())
                .expect("writing to a string cannot fail");
        }
        ExternalCodeOp::Copy => {
            writeln!(
                source,
                "    @evm_extcodecopy({}, {}, {offset}, {len});",
                target.expr(),
                plank_ptr("scratch", EXTCODE_OFFSET)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, @evm_keccak256({}, {len}));",
                plank_ptr("scratch", EXTCODE_OFFSET)
            )
            .expect("writing to a string cannot fail");
        }
    }
}

fn render_plank_call(
    source: &mut String,
    kind: CallKind,
    input_len: usize,
    output_len: usize,
    returndata_len: usize,
) {
    writeln!(source, "    @mstore32({}, acc);", plank_ptr("scratch", CALL_INPUT_OFFSET))
        .expect("writing to a string cannot fail");
    let call = match kind {
        CallKind::CallEcho => format!(
            "@evm_call(100000, {HELPER_ECHO_WORD}, 0, {}, {input_len}, {}, {output_len})",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::StaticEcho => format!(
            "@evm_staticcall(100000, {HELPER_ECHO_WORD}, {}, {input_len}, {}, {output_len})",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::Revert => format!(
            "@evm_call(100000, {HELPER_REVERT_WORD}, 0, {}, {input_len}, {}, {output_len})",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::DelegateEcho => format!(
            "@evm_delegatecall(100000, {HELPER_ECHO_WORD}, {}, {input_len}, {}, {output_len})",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
    };
    writeln!(source, "    if {call} {{").expect("writing to a string cannot fail");
    writeln!(
        source,
        "        acc = @evm_xor(acc, @mload32({}));",
        plank_ptr("scratch", CALL_OUTPUT_OFFSET)
    )
    .expect("writing to a string cannot fail");
    source.push_str("    } else {\n");
    source.push_str("        acc = @evm_xor(acc, 0xdead);\n");
    source.push_str("    }\n");
    source.push_str("    acc = @evm_xor(acc, @evm_returndatasize());\n");
    if returndata_len > 0 {
        writeln!(
            source,
            "    @evm_returndatacopy({}, 0, {returndata_len});",
            plank_ptr("scratch", RETURN_DATA_OFFSET)
        )
        .expect("writing to a string cannot fail");
        writeln!(
            source,
            "    acc = @evm_xor(acc, @evm_keccak256({}, {returndata_len}));",
            plank_ptr("scratch", RETURN_DATA_OFFSET)
        )
        .expect("writing to a string cannot fail");
    }
}

fn render_plank_exit(source: &mut String, cfg: &EntryConfig) {
    match cfg.exit.kind {
        ExitKind::Return => {
            render_plank_output(source, cfg.exit.output_len, cfg, 1);
            writeln!(source, "    @evm_return(scratch, {});", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::Revert => {
            render_plank_output(source, cfg.exit.output_len, cfg, 1);
            writeln!(source, "    @evm_revert(scratch, {});", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::ConditionalReturnRevert => {
            source.push_str("    if @evm_iszero(@evm_and(acc, 1)) {\n");
            render_plank_output(source, cfg.exit.output_len, cfg, 2);
            writeln!(source, "        @evm_return(scratch, {});", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
            source.push_str("    } else {\n");
            render_plank_output(source, cfg.exit.output_len, cfg, 2);
            writeln!(source, "        @evm_revert(scratch, {});", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
            source.push_str("    }\n");
        }
        ExitKind::Stop => source.push_str("    @evm_stop();\n"),
        ExitKind::Invalid => source.push_str("    @evm_invalid();\n"),
    }
}

fn render_plank_output(source: &mut String, len: usize, cfg: &EntryConfig, indent: usize) {
    let indent = "    ".repeat(indent);
    for word in 0..words_for_len(len) {
        writeln!(
            source,
            "{indent}@mstore32({}, @evm_add(acc, {}));",
            plank_ptr("scratch", word * 32),
            cfg.constants.expr(word + 7)
        )
        .expect("writing to a string cannot fail");
    }
}

fn render_yul_entry(source: &mut String, entry_index: usize, entry: &Entry, mode: ProgramMode) {
    writeln!(source, "            function entry_{entry_index}() {{")
        .expect("writing to a string cannot fail");
    writeln!(source, "                let scratch := mload(0x40)")
        .expect("writing to a string cannot fail");
    writeln!(source, "                mstore(0x40, add(scratch, {SCRATCH_BYTES}))")
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "                let acc := xor(calldataload({}), {})",
        calldata_offset(mode, 0),
        entry.config.constants.expr(0)
    )
    .expect("writing to a string cannot fail");

    for (fragment_index, fragment) in entry.config.fragments.iter().enumerate() {
        render_yul_fragment(source, entry_index, fragment_index, fragment, &entry.config);
    }

    render_yul_exit(source, &entry.config);
    source.push_str("            }\n");
}

fn render_yul_fragment(
    source: &mut String,
    entry_index: usize,
    fragment_index: usize,
    fragment: &Fragment,
    cfg: &EntryConfig,
) {
    match fragment {
        Fragment::Calldata { op, offset, len, dst } => match op {
            CalldataOp::Size => {
                source.push_str("                acc := xor(acc, calldatasize())\n")
            }
            CalldataOp::Load => {
                writeln!(source, "                acc := xor(acc, calldataload({offset}))")
                    .expect("writing to a string cannot fail");
            }
            CalldataOp::Copy => {
                writeln!(
                    source,
                    "                calldatacopy({}, {offset}, {len})",
                    yul_ptr("scratch", *dst)
                )
                .expect("writing to a string cannot fail");
                writeln!(
                    source,
                    "                acc := xor(acc, keccak256({}, {len}))",
                    yul_ptr("scratch", *dst)
                )
                .expect("writing to a string cannot fail");
            }
        },
        Fragment::Arithmetic { op, lhs, rhs, aux } => {
            render_yul_arithmetic(source, *op, cfg, *lhs, *rhs, *aux);
        }
        Fragment::Memory { width, offset, value } => {
            render_yul_memory_width(
                source,
                fragment_index,
                *width,
                *offset,
                &cfg.constants.expr(*value),
            );
        }
        Fragment::MemoryCopy { dst, src, len } => {
            writeln!(
                source,
                "                mcopy({}, {}, {len})",
                yul_ptr("scratch", *dst),
                yul_ptr("scratch", *src)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                acc := xor(acc, keccak256({}, {len}))",
                yul_ptr("scratch", *dst)
            )
            .expect("writing to a string cannot fail");
        }
        Fragment::Storage { slot, repeated } => {
            let storage_slot = storage_slot(entry_index, *slot, cfg);
            writeln!(source, "                acc := xor(acc, sload({}))", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                sstore({}, xor(acc, {}))",
                hex_u64(storage_slot),
                cfg.constants.expr(*slot)
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "                acc := add(acc, sload({}))", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
            if *repeated {
                writeln!(
                    source,
                    "                sstore({}, xor(acc, {}))",
                    hex_u64(storage_slot),
                    cfg.constants.expr(*slot + 1)
                )
                .expect("writing to a string cannot fail");
                writeln!(
                    source,
                    "                acc := xor(acc, sload({}))",
                    hex_u64(storage_slot)
                )
                .expect("writing to a string cannot fail");
            }
        }
        Fragment::TransientStorage { slot } => {
            let storage_slot = storage_slot(entry_index, *slot, cfg);
            writeln!(
                source,
                "                tstore({}, xor(acc, {}))",
                hex_u64(storage_slot),
                cfg.constants.expr(*slot)
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "                acc := xor(acc, tload({}))", hex_u64(storage_slot))
                .expect("writing to a string cannot fail");
        }
        Fragment::Environment { op } => render_yul_environment(source, *op),
        Fragment::ExternalCode { op, target, offset, len } => {
            render_yul_external_code(source, *op, *target, *offset, *len);
        }
        Fragment::Call { kind, input_len, output_len, returndata_len } => {
            render_yul_call(source, *kind, *input_len, *output_len, *returndata_len);
        }
        Fragment::Create { kind, salt } => {
            let ptr = yul_ptr("scratch", CREATE_OFFSET);
            writeln!(source, "                mstore({ptr}, {CREATE_INIT_WORD})")
                .expect("writing to a string cannot fail");
            match kind {
                CreateKind::Create => {
                    writeln!(
                        source,
                        "                let created_{fragment_index} := create(0, {ptr}, {CREATE_INIT_BYTES})"
                    )
                    .expect("writing to a string cannot fail");
                }
                CreateKind::Create2 => {
                    writeln!(
                        source,
                        "                let created_{fragment_index} := create2(0, {ptr}, {CREATE_INIT_BYTES}, {})",
                        cfg.constants.expr(*salt)
                    )
                    .expect("writing to a string cannot fail");
                }
            }
            writeln!(source, "                acc := xor(acc, created_{fragment_index})")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                acc := xor(acc, extcodesize(created_{fragment_index}))"
            )
            .expect("writing to a string cannot fail");
        }
        Fragment::Log { topics, len } => {
            for word in 0..words_for_len(*len) {
                writeln!(
                    source,
                    "                mstore({}, add(acc, {}))",
                    yul_ptr("scratch", word * 32),
                    cfg.constants.expr(word)
                )
                .expect("writing to a string cannot fail");
            }
            let mut args = vec!["scratch".to_string(), len.to_string()];
            args.extend((0..*topics).map(|topic| yul_topic_expr(cfg, topic)));
            writeln!(source, "                log{}({})", topics, args.join(", "))
                .expect("writing to a string cannot fail");
        }
        Fragment::Branch { left, right } => {
            source.push_str("                switch iszero(and(acc, 1))\n");
            writeln!(
                source,
                "                case 0 {{ acc := xor(acc, {}) }}",
                cfg.constants.expr(*right)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                default {{ acc := add(acc, {}) }}",
                cfg.constants.expr(*left)
            )
            .expect("writing to a string cannot fail");
        }
        Fragment::Loop { iterations, value } => {
            writeln!(
                source,
                "                for {{ let i := 0 }} lt(i, {iterations}) {{ i := add(i, 1) }} {{"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                    acc := add(acc, xor(i, {}))",
                cfg.constants.expr(*value)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                    mstore({}, acc)",
                yul_ptr("scratch", LOOP_MEMORY_OFFSET)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                    acc := xor(acc, mload({}))",
                yul_ptr("scratch", LOOP_MEMORY_OFFSET)
            )
            .expect("writing to a string cannot fail");
            source.push_str("                }\n");
        }
    }
}

fn render_yul_arithmetic(
    source: &mut String,
    op: ArithmeticOp,
    cfg: &EntryConfig,
    lhs: usize,
    rhs: usize,
    aux: usize,
) {
    let left = cfg.constants.expr(lhs);
    let right = cfg.constants.expr(rhs);
    match op {
        ArithmeticOp::Add => writeln!(source, "                acc := add(acc, {right})"),
        ArithmeticOp::Sub => writeln!(source, "                acc := sub(acc, {right})"),
        ArithmeticOp::Mul => writeln!(source, "                acc := mul(acc, {right})"),
        ArithmeticOp::Div => writeln!(source, "                acc := div(acc, {right})"),
        ArithmeticOp::SDiv => writeln!(source, "                acc := sdiv(acc, {right})"),
        ArithmeticOp::Mod => writeln!(source, "                acc := mod(acc, {right})"),
        ArithmeticOp::SMod => writeln!(source, "                acc := smod(acc, {right})"),
        ArithmeticOp::AddMod => {
            writeln!(source, "                acc := addmod(acc, {left}, {right})")
        }
        ArithmeticOp::MulMod => {
            writeln!(source, "                acc := mulmod(acc, {left}, {right})")
        }
        ArithmeticOp::Exp => {
            writeln!(source, "                acc := exp(acc, and({right}, 0xff))")
        }
        ArithmeticOp::SignExtend => {
            writeln!(source, "                acc := signextend({}, acc)", aux % 32)
        }
        ArithmeticOp::Lt => writeln!(source, "                acc := xor(acc, lt(acc, {right}))"),
        ArithmeticOp::Gt => writeln!(source, "                acc := xor(acc, gt(acc, {right}))"),
        ArithmeticOp::SLt => writeln!(source, "                acc := xor(acc, slt(acc, {right}))"),
        ArithmeticOp::SGt => writeln!(source, "                acc := xor(acc, sgt(acc, {right}))"),
        ArithmeticOp::Eq => writeln!(source, "                acc := xor(acc, eq(acc, {right}))"),
        ArithmeticOp::IsZero => writeln!(source, "                acc := xor(acc, iszero(acc))"),
        ArithmeticOp::And => writeln!(source, "                acc := and(acc, {right})"),
        ArithmeticOp::Or => writeln!(source, "                acc := or(acc, {right})"),
        ArithmeticOp::Xor => writeln!(source, "                acc := xor(acc, {right})"),
        ArithmeticOp::Not => writeln!(source, "                acc := not(acc)"),
        ArithmeticOp::Byte => {
            writeln!(source, "                acc := xor(acc, byte({}, {right}))", aux % 32)
        }
        ArithmeticOp::Shl => writeln!(source, "                acc := shl({aux}, acc)"),
        ArithmeticOp::Shr => writeln!(source, "                acc := shr({aux}, acc)"),
        ArithmeticOp::Sar => writeln!(source, "                acc := sar({aux}, acc)"),
    }
    .expect("writing to a string cannot fail");
}

fn render_yul_memory_width(
    source: &mut String,
    fragment_index: usize,
    width: usize,
    offset: usize,
    value: &str,
) {
    let ptr = yul_ptr("scratch", offset);
    if width == 32 {
        writeln!(source, "                mstore({ptr}, add(acc, {value}))")
            .expect("writing to a string cannot fail");
        writeln!(source, "                acc := xor(acc, mload({ptr}))")
            .expect("writing to a string cannot fail");
        return;
    }

    if width == 1 {
        writeln!(source, "                mstore8({ptr}, add(acc, {value}))")
            .expect("writing to a string cannot fail");
        writeln!(source, "                acc := xor(acc, byte(0, mload({ptr})))")
            .expect("writing to a string cannot fail");
        return;
    }

    let bits = width * 8;
    let tail_bits = 256 - bits;
    writeln!(source, "                let tail_{fragment_index} := mload({ptr})")
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "                tail_{fragment_index} := shr({bits}, shl({bits}, tail_{fragment_index}))"
    )
    .expect("writing to a string cannot fail");
    writeln!(
        source,
        "                mstore({ptr}, or(tail_{fragment_index}, shl({tail_bits}, add(acc, {value}))))"
    )
    .expect("writing to a string cannot fail");
    writeln!(source, "                acc := xor(acc, shr({tail_bits}, mload({ptr})))")
        .expect("writing to a string cannot fail");
}

fn render_yul_environment(source: &mut String, op: EnvironmentOp) {
    match op {
        EnvironmentOp::Address => source.push_str("                acc := xor(acc, address())\n"),
        EnvironmentOp::Balance => {
            writeln!(source, "                acc := xor(acc, balance({HELPER_ECHO_WORD}))")
                .expect("writing to a string cannot fail");
        }
        EnvironmentOp::Origin => source.push_str("                acc := xor(acc, origin())\n"),
        EnvironmentOp::Caller => source.push_str("                acc := xor(acc, caller())\n"),
        EnvironmentOp::CallValue => {
            source.push_str("                acc := xor(acc, callvalue())\n")
        }
        EnvironmentOp::GasPrice => source.push_str("                acc := xor(acc, gasprice())\n"),
        EnvironmentOp::BlockHash => {
            source.push_str("                acc := xor(acc, blockhash(sub(number(), 1)))\n");
        }
        EnvironmentOp::Coinbase => source.push_str("                acc := xor(acc, coinbase())\n"),
        EnvironmentOp::Timestamp => {
            source.push_str("                acc := xor(acc, timestamp())\n")
        }
        EnvironmentOp::Number => source.push_str("                acc := xor(acc, number())\n"),
        EnvironmentOp::Difficulty => {
            source.push_str("                acc := xor(acc, prevrandao())\n")
        }
        EnvironmentOp::GasLimit => source.push_str("                acc := xor(acc, gaslimit())\n"),
        EnvironmentOp::ChainId => source.push_str("                acc := xor(acc, chainid())\n"),
        EnvironmentOp::SelfBalance => {
            source.push_str("                acc := xor(acc, selfbalance())\n")
        }
        EnvironmentOp::BaseFee => source.push_str("                acc := xor(acc, basefee())\n"),
    }
}

fn render_yul_external_code(
    source: &mut String,
    op: ExternalCodeOp,
    target: AddressTarget,
    offset: usize,
    len: usize,
) {
    match op {
        ExternalCodeOp::Size => {
            writeln!(source, "                acc := xor(acc, extcodesize({}))", target.expr())
                .expect("writing to a string cannot fail");
        }
        ExternalCodeOp::Hash => {
            writeln!(source, "                acc := xor(acc, extcodehash({}))", target.expr())
                .expect("writing to a string cannot fail");
        }
        ExternalCodeOp::Copy => {
            writeln!(
                source,
                "                extcodecopy({}, {}, {offset}, {len})",
                target.expr(),
                yul_ptr("scratch", EXTCODE_OFFSET)
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "                acc := xor(acc, keccak256({}, {len}))",
                yul_ptr("scratch", EXTCODE_OFFSET)
            )
            .expect("writing to a string cannot fail");
        }
    }
}

fn render_yul_call(
    source: &mut String,
    kind: CallKind,
    input_len: usize,
    output_len: usize,
    returndata_len: usize,
) {
    writeln!(source, "                mstore({}, acc)", yul_ptr("scratch", CALL_INPUT_OFFSET))
        .expect("writing to a string cannot fail");
    let call = match kind {
        CallKind::CallEcho => format!(
            "call(100000, {HELPER_ECHO_WORD}, 0, {}, {input_len}, {}, {output_len})",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::StaticEcho => format!(
            "staticcall(100000, {HELPER_ECHO_WORD}, {}, {input_len}, {}, {output_len})",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::Revert => format!(
            "call(100000, {HELPER_REVERT_WORD}, 0, {}, {input_len}, {}, {output_len})",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::DelegateEcho => format!(
            "delegatecall(100000, {HELPER_ECHO_WORD}, {}, {input_len}, {}, {output_len})",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
    };
    writeln!(source, "                switch {call}").expect("writing to a string cannot fail");
    source.push_str("                case 0 { acc := xor(acc, 0xdead) }\n");
    writeln!(
        source,
        "                default {{ acc := xor(acc, mload({})) }}",
        yul_ptr("scratch", CALL_OUTPUT_OFFSET)
    )
    .expect("writing to a string cannot fail");
    source.push_str("                acc := xor(acc, returndatasize())\n");
    if returndata_len > 0 {
        writeln!(
            source,
            "                returndatacopy({}, 0, {returndata_len})",
            yul_ptr("scratch", RETURN_DATA_OFFSET)
        )
        .expect("writing to a string cannot fail");
        writeln!(
            source,
            "                acc := xor(acc, keccak256({}, {returndata_len}))",
            yul_ptr("scratch", RETURN_DATA_OFFSET)
        )
        .expect("writing to a string cannot fail");
    }
}

fn render_yul_exit(source: &mut String, cfg: &EntryConfig) {
    match cfg.exit.kind {
        ExitKind::Return => {
            render_yul_output(source, cfg.exit.output_len, cfg, 4);
            writeln!(source, "                return(scratch, {})", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::Revert => {
            render_yul_output(source, cfg.exit.output_len, cfg, 4);
            writeln!(source, "                revert(scratch, {})", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::ConditionalReturnRevert => {
            source.push_str("                switch iszero(and(acc, 1))\n");
            source.push_str("                case 0 {\n");
            render_yul_output(source, cfg.exit.output_len, cfg, 5);
            writeln!(source, "                    revert(scratch, {})", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
            source.push_str("                }\n");
            source.push_str("                default {\n");
            render_yul_output(source, cfg.exit.output_len, cfg, 5);
            writeln!(source, "                    return(scratch, {})", cfg.exit.output_len)
                .expect("writing to a string cannot fail");
            source.push_str("                }\n");
        }
        ExitKind::Stop => source.push_str("                stop()\n"),
        ExitKind::Invalid => source.push_str("                invalid()\n"),
    }
}

fn render_yul_output(source: &mut String, len: usize, cfg: &EntryConfig, indent: usize) {
    let indent = "    ".repeat(indent);
    for word in 0..words_for_len(len) {
        writeln!(
            source,
            "{indent}mstore({}, add(acc, {}))",
            yul_ptr("scratch", word * 32),
            cfg.constants.expr(word + 7)
        )
        .expect("writing to a string cannot fail");
    }
}

fn unique_selector(
    u: &mut Unstructured<'_>,
    index: usize,
    existing: &[u32],
) -> arbitrary::Result<u32> {
    let mut selector = u32::arbitrary(u)?.wrapping_add(0x1000_0000 ^ index as u32);
    if selector == 0 {
        selector = 0x1000_0001;
    }

    while existing.contains(&selector) {
        selector = selector.wrapping_add(0x0100_0193);
        if selector == 0 {
            selector = 0x1000_0001;
        }
    }

    Ok(selector)
}

fn calldata_offset(mode: ProgramMode, input_index: usize) -> usize {
    let selector_bytes = if mode == ProgramMode::SelectorDispatch { 4 } else { 0 };
    selector_bytes + input_index * 32
}

fn storage_slot(entry_index: usize, slot_index: usize, cfg: &EntryConfig) -> u64 {
    let salt = cfg.constants.words[0][31] as u64;
    0x0100_0000u64 + ((entry_index as u64) << 16) + ((slot_index as u64) << 8) + (salt & 0xff)
}

fn words_for_len(len: usize) -> usize {
    len.div_ceil(32)
}

fn plank_ptr(base: &str, offset: usize) -> String {
    if offset == 0 { base.to_string() } else { format!("{base} +% {offset}") }
}

fn yul_ptr(base: &str, offset: usize) -> String {
    if offset == 0 { base.to_string() } else { format!("add({base}, {offset})") }
}

fn plank_topic_expr(cfg: &EntryConfig, topic: usize) -> String {
    match topic {
        0 => "acc".to_string(),
        1 => format!("@evm_xor(acc, {})", cfg.constants.expr(0)),
        2 => format!("@evm_add(acc, {})", cfg.constants.expr(1)),
        _ => format!("@evm_xor(acc, {})", cfg.constants.expr(topic)),
    }
}

fn yul_topic_expr(cfg: &EntryConfig, topic: usize) -> String {
    match topic {
        0 => "acc".to_string(),
        1 => format!("xor(acc, {})", cfg.constants.expr(0)),
        2 => format!("add(acc, {})", cfg.constants.expr(1)),
        _ => format!("xor(acc, {})", cfg.constants.expr(topic)),
    }
}

fn arbitrary_word(u: &mut Unstructured<'_>) -> arbitrary::Result<[u8; 32]> {
    let mut word = [0u8; 32];
    for byte in &mut word {
        *byte = u8::arbitrary(u)?;
    }
    Ok(word)
}

fn has_full_width_word(bytes: &[u8]) -> bool {
    bytes.chunks(32).any(|chunk| chunk.len() == 32 && chunk[..24].iter().any(|byte| *byte != 0))
}

fn hex_word(bytes: [u8; 32]) -> String {
    format!("0x{}", alloy_primitives::hex::encode(bytes))
}

fn hex_u32(value: u32) -> String {
    format!("0x{value:08x}")
}

fn hex_u64(value: u64) -> String {
    format!("0x{value:x}")
}

#[cfg(test)]
mod tests {
    use super::GeneratedCase;
    use arbitrary::{Arbitrary, Unstructured};

    #[test]
    fn generated_case_renders_rich_plank_and_solidity_sources() {
        let bytes = vec![17; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = GeneratedCase::arbitrary(&mut u).expect("case should decode");

        let plank = case.plank_source();
        let solidity = case.solidity_source();

        assert!(plank.contains("init {"));
        assert!(
            plank.contains("@evm_return")
                || plank.contains("@evm_revert")
                || plank.contains("@evm_stop")
                || plank.contains("@evm_invalid")
        );
        assert!(solidity.contains("contract C {"));
        assert!(solidity.contains("assembly"));
    }

    #[test]
    fn generated_case_exposes_call_sequence() {
        let bytes = vec![91; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = GeneratedCase::arbitrary(&mut u).expect("case should decode");

        assert_eq!(case.call_count(), case.calldatas().len());
        assert!(case.call_count() >= 1);
    }
}
