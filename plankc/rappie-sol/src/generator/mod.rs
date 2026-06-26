use crate::{
    evm::{
        CALLER_WORD, EMPTY_ACCOUNT_WORD, HELPER_CODE_WORD, HELPER_ECHO_WORD, HELPER_REVERT_WORD,
    },
    sources::{PlankSourceFile, PlankSourceSet, StdMode},
};
use arbitrary::{Arbitrary, Unstructured};
use std::{collections::BTreeSet, fmt::Write, path::PathBuf};

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
const CREATE2_NONCE_SLOT: &str = "0x02000000";

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
    pub has_struct: bool,
    pub has_tuple: bool,
    pub has_compound_literal: bool,
    pub has_field_access: bool,
    pub has_field_update: bool,
    pub has_comptime_type_reflection: bool,
    pub has_cbytes_builtin: bool,
    pub has_high_level_operator: bool,
    pub has_core_ops_operator: bool,
    pub has_helper_function: bool,
    pub has_nested_helper_call: bool,
    pub has_import: bool,
    pub has_import_single: bool,
    pub has_import_group: bool,
    pub has_import_alias: bool,
    pub has_import_glob: bool,
    pub has_deep_import: bool,
    pub has_comments: bool,
    pub has_binary_literal: bool,
    pub has_hex_literal: bool,
    pub has_parameterized_type: bool,
    pub has_comptime_control_flow: bool,
    pub has_comptime_loop: bool,
    pub has_type_dependent_branch: bool,
    pub has_function_returns_compound: bool,
    pub has_function_early_return: bool,
    pub has_nested_compound: bool,
    pub has_runtime_uninit: bool,
    pub has_data_offset: bool,
    pub has_std_registered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedCase {
    mode: ProgramMode,
    entries: Vec<Entry>,
    calls: Vec<CallStep>,
    frontend: FrontendPlan,
}

impl GeneratedCase {
    pub(crate) fn plank_source(&self) -> String {
        let mut source = String::new();

        self.frontend.render_main_header(&mut source);

        if self.mode == ProgramMode::SelectorDispatch {
            for (index, entry) in self.entries.iter().enumerate() {
                writeln!(source, "const SELECTOR_{index} = {};", hex_u32(entry.selector))
                    .expect("writing to a string cannot fail");
            }
            source.push('\n');
        }

        for (index, entry) in self.entries.iter().enumerate() {
            render_plank_entry(&mut source, index, entry, self.mode, &self.frontend);
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

    pub(crate) fn plank_sources(&self) -> PlankSourceSet {
        let mut files = vec![PlankSourceFile::new("main.plk", self.plank_source())];
        if self.frontend.has_import {
            files.push(PlankSourceFile::new("gen/types.plk", self.frontend.render_types_file()));
            files
                .push(PlankSourceFile::new("gen/helpers.plk", self.frontend.render_helpers_file()));
            if self.frontend.has_import_single || self.frontend.has_import_alias {
                files.push(PlankSourceFile::new(
                    "gen/extras.plk",
                    self.frontend.render_extras_file(),
                ));
            }
            if self.frontend.has_import_glob {
                files.push(PlankSourceFile::new(
                    "gen/glob_helpers.plk",
                    self.frontend.render_glob_helpers_file(),
                ));
            }
            if self.frontend.has_deep_import {
                files.push(PlankSourceFile::new(
                    "gen/nested/boxes.plk",
                    self.frontend.render_nested_boxes_file(),
                ));
            }
        }

        PlankSourceSet {
            entry_path: PathBuf::from("main.plk"),
            files,
            std_mode: if self.frontend.has_core_ops_operator {
                StdMode::RepoStd
            } else {
                StdMode::None
            },
        }
    }

    pub(crate) fn solidity_source(&self) -> String {
        let mut source = String::new();
        source.push_str("// SPDX-License-Identifier: MIT\n");
        source.push_str("pragma solidity >=0.8.20;\n\n");
        source.push_str("contract C {\n");
        source.push_str("    fallback() external payable {\n");
        source.push_str("        assembly (\"memory-safe\") {\n");
        self.frontend.render_yul_helper_defs(&mut source);

        for (index, entry) in self.entries.iter().enumerate() {
            render_yul_entry(&mut source, index, entry, self.mode, &self.frontend);
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
            has_std_registered: false,
        };

        self.frontend.classify(&mut classification);

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

        let frontend = FrontendPlan::arbitrary(u)?;

        Ok(Self { mode, entries, calls, frontend })
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FrontendPlan {
    has_struct: bool,
    has_tuple: bool,
    has_comptime_type_reflection: bool,
    has_cbytes_builtin: bool,
    has_high_level_operator: bool,
    has_core_ops_operator: bool,
    has_helper_function: bool,
    has_nested_helper_call: bool,
    has_import: bool,
    has_import_single: bool,
    has_import_alias: bool,
    has_import_glob: bool,
    has_deep_import: bool,
    has_comments: bool,
    has_binary_literal: bool,
    has_hex_literal: bool,
    has_parameterized_type: bool,
    has_comptime_control_flow: bool,
    has_comptime_loop: bool,
    has_type_dependent_branch: bool,
    has_function_returns_compound: bool,
    has_function_early_return: bool,
    has_nested_compound: bool,
    has_runtime_uninit: bool,
    has_data_offset: bool,
}

impl FrontendPlan {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let mask = u.int_in_range(0u32..=0x01ff_ffff)?;
        let mut plan = Self {
            has_struct: mask & (1 << 0) != 0,
            has_tuple: mask & (1 << 1) != 0,
            has_comptime_type_reflection: mask & (1 << 2) != 0,
            has_cbytes_builtin: mask & (1 << 3) != 0,
            has_high_level_operator: mask & (1 << 4) != 0,
            has_core_ops_operator: mask & (1 << 5) != 0,
            has_helper_function: mask & (1 << 6) != 0,
            has_nested_helper_call: mask & (1 << 7) != 0,
            has_import: mask & (1 << 8) != 0,
            has_import_single: mask & (1 << 9) != 0,
            has_import_alias: mask & (1 << 10) != 0,
            has_import_glob: mask & (1 << 11) != 0,
            has_deep_import: mask & (1 << 12) != 0,
            has_comments: mask & (1 << 13) != 0,
            has_binary_literal: mask & (1 << 14) != 0,
            has_hex_literal: mask & (1 << 15) != 0,
            has_parameterized_type: mask & (1 << 16) != 0,
            has_comptime_control_flow: mask & (1 << 17) != 0,
            has_comptime_loop: mask & (1 << 18) != 0,
            has_type_dependent_branch: mask & (1 << 19) != 0,
            has_function_returns_compound: mask & (1 << 20) != 0,
            has_function_early_return: mask & (1 << 21) != 0,
            has_nested_compound: mask & (1 << 22) != 0,
            has_runtime_uninit: mask & (1 << 23) != 0,
            has_data_offset: mask & (1 << 24) != 0,
        };

        if plan.has_import_single
            || plan.has_import_alias
            || plan.has_import_glob
            || plan.has_deep_import
        {
            plan.has_import = true;
        }
        if plan.has_import {
            plan.has_struct = true;
            plan.has_tuple = true;
            plan.has_helper_function = true;
        }
        if plan.has_deep_import {
            plan.has_import_alias = true;
        }
        if plan.has_nested_helper_call {
            plan.has_helper_function = true;
            plan.has_high_level_operator = true;
        }
        if plan.has_comptime_type_reflection {
            plan.has_struct = true;
            plan.has_tuple = true;
        }
        if plan.has_parameterized_type {
            plan.has_struct = true;
            plan.has_tuple = true;
        }
        if plan.has_comptime_loop || plan.has_type_dependent_branch {
            plan.has_comptime_control_flow = true;
        }
        if plan.has_comptime_control_flow {
            plan.has_struct = true;
        }
        if plan.has_type_dependent_branch {
            plan.has_struct = true;
            plan.has_tuple = true;
            plan.has_comptime_type_reflection = true;
        }
        if plan.has_function_returns_compound {
            plan.has_struct = true;
            plan.has_tuple = true;
            plan.has_helper_function = true;
        }
        if plan.has_nested_compound {
            plan.has_struct = true;
            plan.has_tuple = true;
        }
        if plan.has_runtime_uninit {
            plan.has_struct = true;
        }
        if plan.has_cbytes_builtin {
            plan.has_hex_literal = true;
        }
        if plan.has_data_offset {
            plan.has_hex_literal = true;
        }

        Ok(plan)
    }

    fn render_main_header(&self, source: &mut String) {
        if self.has_comments {
            source.push_str("// Generated compiler-coverage feature prelude.\n");
            source.push_str("/* Exercises imports, compounds, comptime data, and operators. */\n");
        }

        if self.has_import {
            let mut type_imports = vec![
                "Pair",
                "Triple",
                "Numeric",
                "IS_PAIR_STRUCT",
                "IS_TRIPLE_TUPLE",
                "FIELD_A_OK",
                "FIELD_INDEX_B",
                "FIELD_TYPE_IS_U256",
                "TYPE_INDEX_NUMERIC",
                "DEFAULT_PAIR_A",
                "ACTIVE_EVM",
                "CONST_COMPTIME",
                "CBYTES_SLICE_OK",
                "CBYTES_READ_WORD",
                "CBYTES_READ_OK",
                "CBYTES_CONCAT_OK",
                "CBYTES_KECCAK_OK",
                "CBYTES_SHA_OK",
                "FRONT_BINARY_LITERAL",
                "FRONT_HEX_LITERAL",
            ];
            if self.has_parameterized_type {
                type_imports.extend([
                    "BoxU256",
                    "BoxPair",
                    "BoxTriple",
                    "BOX_U256_PARAMETERIZED",
                    "BOX_U256_NAME_OK",
                    "BOX_U256_FIELD_TYPE_OK",
                    "BOX_PAIR_FIELD_COUNT",
                ]);
            }
            if self.has_comptime_control_flow {
                type_imports.push("FRONT_COMPTIME_BRANCH_VALUE");
            }
            if self.has_comptime_loop {
                type_imports.push("FRONT_COMPTIME_LOOP_VALUE");
            }
            if self.has_type_dependent_branch {
                type_imports.extend(["FRONT_PAIR_TYPE_SCORE", "FRONT_TRIPLE_TYPE_SCORE"]);
            }
            if self.has_nested_compound {
                type_imports.extend(["NestedOuter", "NestedTuple"]);
            }
            if self.has_runtime_uninit {
                type_imports.push("RuntimeScratch");
            }
            writeln!(source, "import gen::types::{{{}}};", type_imports.join(", "))
                .expect("writing to a string cannot fail");

            let mut helper_imports = vec![
                "mix_pair",
                "make_triple",
                "mix_triple",
                "operator_mix",
                "core_operator_mix",
                "nested_mix",
            ];
            if self.has_function_early_return {
                helper_imports.push("choose_front_value");
            }
            if self.has_function_returns_compound {
                helper_imports.extend(["make_front_pair", "make_front_triple"]);
            }
            if self.has_function_returns_compound && self.has_nested_compound {
                helper_imports.push("make_front_outer");
            }
            writeln!(source, "import gen::helpers::{{{}}};", helper_imports.join(", "))
                .expect("writing to a string cannot fail");

            if self.has_import_single {
                source.push_str("import gen::extras::SINGLE_IMPORT_MARKER;\n");
            }
            if self.has_import_alias {
                source.push_str(
                    "import gen::extras::ALIAS_IMPORT_MARKER as ALIASED_IMPORT_MARKER;\n",
                );
            }
            if self.has_import_glob {
                source.push_str("import gen::glob_helpers::*;\n");
            }
            if self.has_deep_import {
                source.push_str(
                    "import gen::nested::boxes::{DeepBoxU256 as ImportedBoxU256, nested_box_value as imported_box_value};\n",
                );
            }
            source.push('\n');
        } else {
            self.render_type_defs(source);
            self.render_helper_defs(source, false);
        }
    }

    fn render_types_file(&self) -> String {
        let mut source = String::new();
        if self.has_comments {
            source.push_str("// Generated imported type and comptime definitions.\n");
        }
        self.render_type_defs(&mut source);
        source
    }

    fn render_helpers_file(&self) -> String {
        let mut source = String::new();
        if self.has_comments {
            source.push_str("// Generated imported helper functions.\n");
        }
        self.render_helper_defs(&mut source, true);
        source
    }

    fn render_extras_file(&self) -> String {
        let mut source = String::new();
        if self.has_comments {
            source.push_str("// Generated direct and alias import definitions.\n");
        }
        source.push_str("const SINGLE_IMPORT_MARKER = 0x401;\n");
        source.push_str("const ALIAS_IMPORT_MARKER = 0x402;\n");
        source
    }

    fn render_glob_helpers_file(&self) -> String {
        let mut source = String::new();
        if self.has_comments {
            source.push_str("// Generated glob import definitions.\n");
        }
        source.push_str("const GLOB_IMPORT_MARKER = 0x403;\n");
        source.push_str(
            "const glob_import_mix = fn (x: u256) u256 { return x +% GLOB_IMPORT_MARKER; };\n",
        );
        source
    }

    fn render_nested_boxes_file(&self) -> String {
        let mut source = String::new();
        if self.has_comments {
            source.push_str("// Generated nested import definitions.\n");
        }
        source.push_str(
            "const DeepBox = fn (comptime T: type) type {\n    struct { value: T }\n};\n",
        );
        source.push_str("const DeepBoxU256 = DeepBox(u256);\n");
        source.push_str(
            "const nested_box_value = fn (item: DeepBoxU256) u256 { return item.value; };\n",
        );
        source
    }

    fn render_type_defs(&self, source: &mut String) {
        if self.needs_compound_defs() {
            source.push_str("const Pair = struct { a: u256, b: u256 };\n");
            source.push_str("const Triple = tuple { u256, u256, u256 };\n");
            source.push_str("const Numeric = struct 42 { a: u256 };\n");
            if self.has_nested_compound {
                source.push_str(
                    "const NestedOuter = struct { pair: Pair, triple: Triple, flag: bool };\n",
                );
                source
                    .push_str("const NestedTuple = tuple { Pair, tuple { u256, u256 }, u256 };\n");
            }
            if self.has_runtime_uninit {
                source.push_str("const RuntimeScratch = struct { ptr: memptr, len: u256 };\n");
            }
            source.push('\n');
        }

        if self.has_parameterized_type {
            source.push_str(
                "const Box = fn (comptime T: type) type {\n    struct { value: T }\n};\n",
            );
            source.push_str("const BoxU256 = Box(u256);\n");
            source.push_str("const BoxPair = Box(Pair);\n");
            source.push_str("const BoxTriple = Box(Triple);\n");
            source.push_str("const BOX_U256_PARAMETERIZED = @has_parameterized_name(BoxU256);\n");
            source.push_str("const BOX_U256_NAME_OK = @type_name(BoxU256) == \"Box(u256)\";\n");
            source.push_str("const BOX_U256_FIELD_TYPE_OK = @field_type(BoxU256, 0) == u256;\n");
            source.push_str("const BOX_PAIR_FIELD_COUNT = @field_count(BoxPair);\n\n");
        }

        if self.has_comptime_type_reflection {
            source.push_str("const IS_PAIR_STRUCT = @is_struct(Pair);\n");
            source.push_str("const IS_TRIPLE_TUPLE = @is_tuple(Triple);\n");
            source.push_str("const FIELD_A_OK = @field_name(Pair, 0) == \"a\";\n");
            source.push_str("const FIELD_INDEX_B = @field_index(Pair, \"b\");\n");
            source.push_str("const FIELD_TYPE_IS_U256 = @field_type(Pair, 0) == u256;\n");
            source.push_str("const TYPE_INDEX_NUMERIC = @type_index(Numeric);\n");
            source.push_str("const DEFAULT_PAIR = @uninit(Pair);\n");
            source.push_str("const DEFAULT_PAIR_A = DEFAULT_PAIR.a;\n");
            source.push_str("const ACTIVE_EVM = @active_evm_version();\n");
            source.push_str("const CONST_COMPTIME = @in_comptime();\n\n");
        } else if self.has_import {
            source.push_str("const IS_PAIR_STRUCT = true;\n");
            source.push_str("const IS_TRIPLE_TUPLE = true;\n");
            source.push_str("const FIELD_A_OK = true;\n");
            source.push_str("const FIELD_INDEX_B = 1;\n");
            source.push_str("const FIELD_TYPE_IS_U256 = true;\n");
            source.push_str("const TYPE_INDEX_NUMERIC = 42;\n");
            source.push_str("const DEFAULT_PAIR_A = 0;\n");
            source.push_str("const ACTIVE_EVM = 13;\n");
            source.push_str("const CONST_COMPTIME = true;\n\n");
        }

        if self.has_comptime_control_flow {
            source.push_str("const FRONT_COMPTIME_BRANCH_VALUE = comptime {\n");
            source.push_str("    let mut value = 0;\n");
            source.push_str("    if @is_struct(Pair) {\n");
            source.push_str("        value = value +% 0x501;\n");
            source.push_str("    } else {\n");
            source.push_str("        value = value +% 0x502;\n");
            source.push_str("    }\n");
            source.push_str("    value\n");
            source.push_str("};\n\n");
        }

        if self.has_comptime_loop {
            source.push_str("const FRONT_COMPTIME_LOOP_VALUE = comptime {\n");
            source.push_str("    let mut i = 0;\n");
            source.push_str("    let mut value = 0;\n");
            source.push_str("    while i < 4 {\n");
            source.push_str("        value = value +% i;\n");
            source.push_str("        i = i +% 1;\n");
            source.push_str("    }\n");
            source.push_str("    value\n");
            source.push_str("};\n\n");
        }

        if self.has_type_dependent_branch {
            source.push_str("const front_type_score = fn (comptime T: type) u256 {\n");
            source.push_str("    if @is_struct(T) {\n");
            source.push_str("        return @field_count(T) +% 10;\n");
            source.push_str("    }\n");
            source.push_str("    if @is_tuple(T) {\n");
            source.push_str("        return @field_count(T) +% 20;\n");
            source.push_str("    }\n");
            source.push_str("    return 0;\n");
            source.push_str("};\n");
            source.push_str("const FRONT_PAIR_TYPE_SCORE = front_type_score(Pair);\n");
            source.push_str("const FRONT_TRIPLE_TYPE_SCORE = front_type_score(Triple);\n\n");
        }

        if self.has_cbytes_builtin {
            source.push_str("const CBYTES_SLICE_OK = @slice_cbytes(\"hello\", 1, 4) == \"ell\";\n");
            source.push_str("const CBYTES_READ_WORD = @padded_read_cbytes(hex\"010203\", 1);\n");
            source.push_str("const CBYTES_READ_OK = CBYTES_READ_WORD == 0x0203000000000000000000000000000000000000000000000000000000000000;\n");
            source.push_str("const CBYTES_CONCAT_OK = @concat_cbytes((\"a\", 1, hex\"ff\")) == \"a\" hex\"0000000000000000000000000000000000000000000000000000000000000001ff\";\n");
            source.push_str("const CBYTES_KECCAK_OK = @keccak256_cbytes(\"abc\") == 0x4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45;\n");
            source.push_str("const CBYTES_SHA_OK = @sha256_cbytes(\"abc\") == 0xba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad;\n\n");
        } else if self.has_import {
            source.push_str("const CBYTES_SLICE_OK = true;\n");
            source.push_str("const CBYTES_READ_WORD = 0;\n");
            source.push_str("const CBYTES_READ_OK = true;\n");
            source.push_str("const CBYTES_CONCAT_OK = true;\n");
            source.push_str("const CBYTES_KECCAK_OK = true;\n");
            source.push_str("const CBYTES_SHA_OK = true;\n\n");
        }

        if self.has_binary_literal {
            source.push_str("const FRONT_BINARY_LITERAL = 0b1010_0011;\n");
        } else if self.has_import {
            source.push_str("const FRONT_BINARY_LITERAL = 0;\n");
        }
        if self.has_hex_literal {
            source.push_str("const FRONT_HEX_LITERAL = 0xfeed;\n");
        } else if self.has_import {
            source.push_str("const FRONT_HEX_LITERAL = 0;\n");
        }
        if self.has_binary_literal || self.has_hex_literal || self.has_import {
            source.push('\n');
        }
    }

    fn render_helper_defs(&self, source: &mut String, imported_file: bool) {
        if imported_file {
            if self.has_nested_compound {
                source.push_str("import gen::types::{Pair, Triple, NestedOuter};\n\n");
            } else {
                source.push_str("import gen::types::{Pair, Triple};\n\n");
            }
        }

        if self.has_helper_function || self.has_struct {
            source.push_str(
                "const mix_pair = fn (p: Pair) u256 {\n    let a = p.a;\n    let b = @get_field(p, 1);\n    return a +% @evm_xor(b, 0x44);\n};\n\n",
            );
        }

        if self.has_helper_function || self.has_tuple {
            source.push_str(
                "const make_triple = fn (x: u256, y: u256) Triple {\n    return (x, y, x +% y);\n};\n\n",
            );
            source.push_str(
                "const mix_triple = fn (t: Triple) u256 {\n    let t2 = @set_field(t, 1, @evm_xor(@get_field(t, 1), 0x55));\n    return @get_field(t2, 0) +% @get_field(t2, 1) +% @get_field(t2, 2);\n};\n\n",
            );
        }

        if self.has_high_level_operator || self.has_nested_helper_call {
            source.push_str(
                "const operator_mix = fn (x: u256, y: u256) u256 {\n    let a = x +% y;\n    let b = a -% (y & 0xff);\n    let c = b *% 3;\n    let d = (c | y) ^ (x & 0xff);\n    let e = (d << 1) >> 1;\n    let inv = ~x;\n    let mut bonus = 11;\n    if x < y {\n        bonus = bonus +% 1;\n    }\n    if x > y {\n        bonus = bonus +% 2;\n    }\n    if x == y {\n        bonus = bonus +% 3;\n    }\n    if !(x != y) {\n        bonus = bonus +% 5;\n    }\n    return e +% inv +% bonus;\n};\n\n",
            );
        } else if self.has_import {
            source.push_str(
                "const operator_mix = fn (x: u256, y: u256) u256 { return x +% y; };\n\n",
            );
        }

        if self.has_core_ops_operator {
            source.push_str(
                "const core_operator_mix = fn (x: u256, y: u256) u256 {\n    let safe_x = x & 0xffff;\n    let safe_y = y & 0xff;\n    let checked_sum = safe_x + safe_y;\n    let checked_diff = checked_sum - safe_y;\n    let checked_mul = checked_diff * 3;\n    let checked_mod = checked_mul % 257;\n    let mut bonus = checked_mod;\n    if checked_diff <= checked_sum {\n        bonus = bonus +% 0x31;\n    }\n    if checked_sum >= checked_diff {\n        bonus = bonus +% 0x32;\n    }\n    return bonus;\n};\n\n",
            );
        } else if self.has_helper_function || self.has_import {
            source.push_str(
                "const core_operator_mix = fn (x: u256, y: u256) u256 { return x +% y; };\n\n",
            );
        }

        if self.has_nested_helper_call {
            source.push_str(
                "const nested_mix = fn (x: u256, y: u256) u256 {\n    let t = make_triple(x, y);\n    return operator_mix(mix_triple(t), mix_pair(Pair { a: x, b: y }));\n};\n\n",
            );
        } else if self.has_import {
            source
                .push_str("const nested_mix = fn (x: u256, y: u256) u256 { return x +% y; };\n\n");
        }

        if self.has_function_early_return {
            source.push_str(
                "const choose_front_value = fn (cond: bool, a: u256, b: u256) u256 {\n    if cond {\n        return a;\n    }\n    return b;\n};\n\n",
            );
        }

        if self.has_function_returns_compound {
            source.push_str(
                "const make_front_pair = fn (x: u256, y: u256) Pair {\n    return Pair { a: x, b: y };\n};\n\n",
            );
            source.push_str(
                "const make_front_triple = fn (x: u256, y: u256) Triple {\n    return (x, y, x +% y);\n};\n\n",
            );
            if self.has_nested_compound {
                source.push_str(
                    "const make_front_outer = fn (x: u256, y: u256, flag: bool) NestedOuter {\n    return NestedOuter { pair: Pair { a: x, b: y }, triple: make_front_triple(x, y), flag: flag };\n};\n\n",
                );
            }
        }
    }

    fn render_plank_effects(&self, source: &mut String, entry_index: usize) {
        if self.has_comments {
            writeln!(source, "    // frontend feature effects for entry {entry_index}")
                .expect("writing to a string cannot fail");
        }

        if self.has_struct {
            writeln!(
                source,
                "    let front_pair_{entry_index} = Pair {{ a: acc, b: @evm_xor(acc, 0x1234) }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, mix_pair(front_pair_{entry_index}));")
                .expect("writing to a string cannot fail");
        }

        if self.has_tuple {
            writeln!(
                source,
                "    let front_tuple_{entry_index} = make_triple(acc, @evm_xor(acc, 0x22));"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, mix_triple(front_tuple_{entry_index}));")
                .expect("writing to a string cannot fail");
        }

        if self.has_comptime_type_reflection {
            source
                .push_str("    if IS_PAIR_STRUCT {\n        acc = @evm_xor(acc, 0x101);\n    }\n");
            source
                .push_str("    if IS_TRIPLE_TUPLE {\n        acc = @evm_xor(acc, 0x102);\n    }\n");
            source.push_str("    if FIELD_A_OK {\n        acc = @evm_xor(acc, 0x103);\n    }\n");
            source.push_str(
                "    if FIELD_TYPE_IS_U256 {\n        acc = @evm_xor(acc, 0x104);\n    }\n",
            );
            source
                .push_str("    if CONST_COMPTIME {\n        acc = @evm_xor(acc, 0x105);\n    }\n");
            source.push_str("    acc = @evm_xor(acc, FIELD_INDEX_B);\n");
            source.push_str("    acc = @evm_xor(acc, TYPE_INDEX_NUMERIC);\n");
            source.push_str("    acc = @evm_xor(acc, DEFAULT_PAIR_A);\n");
            source.push_str("    acc = @evm_xor(acc, ACTIVE_EVM);\n");
        }

        if self.has_cbytes_builtin {
            source
                .push_str("    if CBYTES_SLICE_OK {\n        acc = @evm_xor(acc, 0x201);\n    }\n");
            source
                .push_str("    if CBYTES_READ_OK {\n        acc = @evm_xor(acc, 0x202);\n    }\n");
            source.push_str(
                "    if CBYTES_CONCAT_OK {\n        acc = @evm_xor(acc, 0x203);\n    }\n",
            );
            source.push_str(
                "    if CBYTES_KECCAK_OK {\n        acc = @evm_xor(acc, 0x204);\n    }\n",
            );
            source.push_str("    if CBYTES_SHA_OK {\n        acc = @evm_xor(acc, 0x205);\n    }\n");
            source.push_str("    acc = @evm_xor(acc, CBYTES_READ_WORD);\n");
        }

        if self.has_parameterized_type {
            source.push_str(
                "    if BOX_U256_PARAMETERIZED {\n        acc = @evm_xor(acc, 0x301);\n    }\n",
            );
            source.push_str(
                "    if BOX_U256_NAME_OK {\n        acc = @evm_xor(acc, 0x302);\n    }\n",
            );
            source.push_str(
                "    if BOX_U256_FIELD_TYPE_OK {\n        acc = @evm_xor(acc, 0x303);\n    }\n",
            );
            source.push_str("    acc = @evm_xor(acc, BOX_PAIR_FIELD_COUNT);\n");
            writeln!(
                source,
                "    let front_box_{entry_index} = BoxU256 {{ value: @evm_xor(acc, 0x33) }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, front_box_{entry_index}.value);")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let front_box_pair_{entry_index} = BoxPair {{ value: Pair {{ a: acc, b: @evm_xor(acc, 0x34) }} }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, front_box_pair_{entry_index}.value.b);")
                .expect("writing to a string cannot fail");
        }

        if self.has_import_single {
            source.push_str("    acc = @evm_xor(acc, SINGLE_IMPORT_MARKER);\n");
        }
        if self.has_import_alias {
            source.push_str("    acc = @evm_xor(acc, ALIASED_IMPORT_MARKER);\n");
        }
        if self.has_import_glob {
            source.push_str("    acc = @evm_xor(acc, glob_import_mix(acc));\n");
        }
        if self.has_deep_import {
            writeln!(
                source,
                "    let imported_box_{entry_index} = ImportedBoxU256 {{ value: @evm_xor(acc, 0x404) }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, imported_box_value(imported_box_{entry_index}));"
            )
            .expect("writing to a string cannot fail");
        }

        if self.has_comptime_control_flow {
            source.push_str("    acc = @evm_xor(acc, FRONT_COMPTIME_BRANCH_VALUE);\n");
        }
        if self.has_comptime_loop {
            source.push_str("    acc = @evm_xor(acc, FRONT_COMPTIME_LOOP_VALUE);\n");
        }
        if self.has_type_dependent_branch {
            source.push_str("    acc = @evm_xor(acc, FRONT_PAIR_TYPE_SCORE);\n");
            source.push_str("    acc = @evm_xor(acc, FRONT_TRIPLE_TYPE_SCORE);\n");
        }

        if self.has_function_early_return {
            source.push_str(
                "    acc = @evm_xor(acc, choose_front_value(@evm_iszero(@evm_and(acc, 1)), acc, @evm_xor(acc, 0x61)));\n",
            );
        }

        if self.has_function_returns_compound {
            writeln!(
                source,
                "    let front_fn_pair_{entry_index} = make_front_pair(acc, @evm_xor(acc, 0x62));"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, mix_pair(front_fn_pair_{entry_index}));")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let front_fn_triple_{entry_index} = make_front_triple(acc, @evm_xor(acc, 0x63));"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, mix_triple(front_fn_triple_{entry_index}));")
                .expect("writing to a string cannot fail");
            if self.has_nested_compound {
                writeln!(
                    source,
                    "    let front_fn_outer_{entry_index} = make_front_outer(acc, @evm_xor(acc, 0x64), @evm_iszero(@evm_and(acc, 1)));"
                )
                .expect("writing to a string cannot fail");
                writeln!(source, "    acc = @evm_xor(acc, front_fn_outer_{entry_index}.pair.b);")
                    .expect("writing to a string cannot fail");
                writeln!(
                    source,
                    "    acc = @evm_xor(acc, @get_field(front_fn_outer_{entry_index}.triple, 2));"
                )
                .expect("writing to a string cannot fail");
                writeln!(
                    source,
                    "    if front_fn_outer_{entry_index}.flag {{\n        acc = @evm_xor(acc, 0x605);\n    }}"
                )
                .expect("writing to a string cannot fail");
            }
        }

        if self.has_nested_compound {
            writeln!(
                source,
                "    let nested_pair_{entry_index} = Pair {{ a: acc, b: @evm_xor(acc, 0x77) }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_triple_{entry_index} = make_triple(@evm_add(acc, 1), @evm_add(acc, 2));"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_outer_{entry_index} = NestedOuter {{ pair: nested_pair_{entry_index}, triple: nested_triple_{entry_index}, flag: @evm_iszero(@evm_and(acc, 1)) }};"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_outer_updated_{entry_index} = @set_field(nested_outer_{entry_index}, 0, Pair {{ a: nested_outer_{entry_index}.pair.a +% 3, b: @get_field(nested_outer_{entry_index}.pair, 1) }});"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, nested_outer_updated_{entry_index}.pair.a);")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, @get_field(nested_outer_updated_{entry_index}.triple, 2));"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_tuple_{entry_index}: NestedTuple = (Pair {{ a: acc, b: @evm_xor(acc, 0x88) }}, (acc, @evm_xor(acc, 0x99)), acc +% 5);"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_tuple_pair_{entry_index} = @get_field(nested_tuple_{entry_index}, 0);"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let nested_tuple_inner_{entry_index} = @get_field(nested_tuple_{entry_index}, 1);"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, nested_tuple_pair_{entry_index}.b);")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    acc = @evm_xor(acc, @get_field(nested_tuple_inner_{entry_index}, 1));"
            )
            .expect("writing to a string cannot fail");
        }

        if self.has_runtime_uninit {
            writeln!(source, "    let front_uninit_{entry_index} = @uninit(RuntimeScratch);")
                .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let front_uninit_ptr_{entry_index} = @set_field(front_uninit_{entry_index}, 0, scratch);"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    let front_uninit_filled_{entry_index} = @set_field(front_uninit_ptr_{entry_index}, 1, 32);"
            )
            .expect("writing to a string cannot fail");
            writeln!(source, "    acc = @evm_xor(acc, front_uninit_filled_{entry_index}.len);")
                .expect("writing to a string cannot fail");
        }

        if self.has_data_offset {
            writeln!(
                source,
                "    let front_data_offset_{entry_index} = @data_offset(@slice_cbytes(\"hello\" hex\"00ff\", 2, 6));"
            )
            .expect("writing to a string cannot fail");
            writeln!(
                source,
                "    if @evm_eq(front_data_offset_{entry_index}, front_data_offset_{entry_index}) {{\n        acc = @evm_xor(acc, 0x701);\n    }}"
            )
            .expect("writing to a string cannot fail");
        }

        if self.has_high_level_operator {
            source.push_str("    acc = @evm_xor(acc, operator_mix(acc, @evm_calldatasize()));\n");
        }

        if self.has_core_ops_operator {
            source.push_str(
                "    acc = @evm_xor(acc, core_operator_mix(acc, @evm_calldatasize()));\n",
            );
        }

        if self.has_nested_helper_call {
            source.push_str("    acc = @evm_xor(acc, nested_mix(acc, @evm_calldatasize()));\n");
        } else if self.has_helper_function && !(self.has_struct || self.has_tuple) {
            source.push_str("    acc = @evm_xor(acc, core_operator_mix(acc, 7));\n");
        }

        if self.has_binary_literal {
            source.push_str("    acc = @evm_xor(acc, FRONT_BINARY_LITERAL);\n");
        }
        if self.has_hex_literal {
            source.push_str("    acc = @evm_xor(acc, FRONT_HEX_LITERAL);\n");
        }
    }

    fn render_yul_effects(&self, source: &mut String, entry_index: usize) {
        if self.has_comments {
            writeln!(source, "                // frontend feature effects for entry {entry_index}")
                .expect("writing to a string cannot fail");
        }

        if self.has_struct {
            source.push_str("                {\n");
            source.push_str("                    let front_a := acc\n");
            source.push_str("                    let front_b := xor(acc, 0x1234)\n");
            source.push_str(
                "                    let front_mix := add(front_a, xor(front_b, 0x44))\n",
            );
            source.push_str("                    acc := xor(acc, front_mix)\n");
            source.push_str("                }\n");
        }

        if self.has_tuple {
            source.push_str("                {\n");
            source.push_str("                    let t0 := acc\n");
            source.push_str("                    let t1 := xor(acc, 0x22)\n");
            source.push_str("                    let t2 := add(t0, t1)\n");
            source.push_str("                    let t1b := xor(t1, 0x55)\n");
            source.push_str("                    let tuple_mix := add(add(t0, t1b), t2)\n");
            source.push_str("                    acc := xor(acc, tuple_mix)\n");
            source.push_str("                }\n");
        }

        if self.has_comptime_type_reflection {
            source.push_str("                acc := xor(acc, 0x101)\n");
            source.push_str("                acc := xor(acc, 0x102)\n");
            source.push_str("                acc := xor(acc, 0x103)\n");
            source.push_str("                acc := xor(acc, 0x104)\n");
            source.push_str("                acc := xor(acc, 0x105)\n");
            source.push_str("                acc := xor(acc, 1)\n");
            source.push_str("                acc := xor(acc, 42)\n");
            source.push_str("                acc := xor(acc, 0)\n");
            source.push_str("                acc := xor(acc, 13)\n");
        }

        if self.has_cbytes_builtin {
            source.push_str("                acc := xor(acc, 0x201)\n");
            source.push_str("                acc := xor(acc, 0x202)\n");
            source.push_str("                acc := xor(acc, 0x203)\n");
            source.push_str("                acc := xor(acc, 0x204)\n");
            source.push_str("                acc := xor(acc, 0x205)\n");
            source.push_str("                acc := xor(acc, 0x0203000000000000000000000000000000000000000000000000000000000000)\n");
        }

        if self.has_parameterized_type {
            source.push_str("                acc := xor(acc, 0x301)\n");
            source.push_str("                acc := xor(acc, 0x302)\n");
            source.push_str("                acc := xor(acc, 0x303)\n");
            source.push_str("                acc := xor(acc, 1)\n");
            source.push_str("                {\n");
            source.push_str("                    let front_box_value := xor(acc, 0x33)\n");
            source.push_str("                    acc := xor(acc, front_box_value)\n");
            source.push_str("                    let front_box_pair_b := xor(acc, 0x34)\n");
            source.push_str("                    acc := xor(acc, front_box_pair_b)\n");
            source.push_str("                }\n");
        }

        if self.has_import_single {
            source.push_str("                acc := xor(acc, 0x401)\n");
        }
        if self.has_import_alias {
            source.push_str("                acc := xor(acc, 0x402)\n");
        }
        if self.has_import_glob {
            source.push_str("                acc := xor(acc, add(acc, 0x403))\n");
        }
        if self.has_deep_import {
            source.push_str("                acc := xor(acc, xor(acc, 0x404))\n");
        }

        if self.has_comptime_control_flow {
            source.push_str("                acc := xor(acc, 0x501)\n");
        }
        if self.has_comptime_loop {
            source.push_str("                acc := xor(acc, 6)\n");
        }
        if self.has_type_dependent_branch {
            source.push_str("                acc := xor(acc, 12)\n");
            source.push_str("                acc := xor(acc, 23)\n");
        }

        if self.has_function_early_return {
            source.push_str("                {\n");
            source.push_str("                    let chosen := xor(acc, 0x61)\n");
            source.push_str("                    if iszero(and(acc, 1)) { chosen := acc }\n");
            source.push_str("                    acc := xor(acc, chosen)\n");
            source.push_str("                }\n");
        }

        if self.has_function_returns_compound {
            source.push_str("                {\n");
            source.push_str("                    let fn_pair_a := acc\n");
            source.push_str("                    let fn_pair_b := xor(acc, 0x62)\n");
            source.push_str(
                "                    let fn_pair_mix := add(fn_pair_a, xor(fn_pair_b, 0x44))\n",
            );
            source.push_str("                    acc := xor(acc, fn_pair_mix)\n");
            source.push_str("                    let fn_t0 := acc\n");
            source.push_str("                    let fn_t1 := xor(acc, 0x63)\n");
            source.push_str("                    let fn_t2 := add(fn_t0, fn_t1)\n");
            source.push_str("                    let fn_t1b := xor(fn_t1, 0x55)\n");
            source.push_str(
                "                    let fn_tuple_mix := add(add(fn_t0, fn_t1b), fn_t2)\n",
            );
            source.push_str("                    acc := xor(acc, fn_tuple_mix)\n");
            if self.has_nested_compound {
                source.push_str("                    let front_outer_x := acc\n");
                source.push_str("                    let front_outer_y := xor(acc, 0x64)\n");
                source.push_str(
                    "                    let front_outer_t2 := add(front_outer_x, front_outer_y)\n",
                );
                source
                    .push_str("                    let front_outer_flag := iszero(and(acc, 1))\n");
                source.push_str("                    acc := xor(acc, front_outer_y)\n");
                source.push_str("                    acc := xor(acc, front_outer_t2)\n");
                source.push_str(
                    "                    if front_outer_flag { acc := xor(acc, 0x605) }\n",
                );
            }
            source.push_str("                }\n");
        }

        if self.has_nested_compound {
            source.push_str("                {\n");
            source.push_str("                    let nested_pair_a := acc\n");
            source.push_str("                    let nested_pair_b := xor(acc, 0x77)\n");
            source.push_str("                    let nested_t0 := add(acc, 1)\n");
            source.push_str("                    let nested_t1 := add(acc, 2)\n");
            source.push_str("                    let nested_t2 := add(nested_t0, nested_t1)\n");
            source.push_str(
                "                    let nested_updated_pair_a := add(nested_pair_a, 3)\n",
            );
            source.push_str("                    acc := xor(acc, nested_updated_pair_a)\n");
            source.push_str("                    acc := xor(acc, nested_t2)\n");
            source.push_str("                    let nested_tuple_pair_b := xor(acc, 0x88)\n");
            source.push_str("                    let nested_tuple_inner_1 := xor(acc, 0x99)\n");
            source.push_str("                    acc := xor(acc, nested_tuple_pair_b)\n");
            source.push_str("                    acc := xor(acc, nested_tuple_inner_1)\n");
            source.push_str("                    pop(nested_pair_b)\n");
            source.push_str("                }\n");
        }

        if self.has_runtime_uninit {
            source.push_str("                acc := xor(acc, 32)\n");
        }

        if self.has_data_offset {
            source.push_str("                acc := xor(acc, 0x701)\n");
        }

        if self.has_high_level_operator {
            source.push_str(
                "                acc := xor(acc, yul_operator_mix(acc, calldatasize()))\n",
            );
        }

        if self.has_core_ops_operator {
            source.push_str(
                "                acc := xor(acc, yul_core_operator_mix(acc, calldatasize()))\n",
            );
        }

        if self.has_nested_helper_call {
            source
                .push_str("                acc := xor(acc, yul_nested_mix(acc, calldatasize()))\n");
        } else if self.has_helper_function && !(self.has_struct || self.has_tuple) {
            if self.has_core_ops_operator {
                source.push_str("                acc := xor(acc, yul_core_operator_mix(acc, 7))\n");
            } else {
                source.push_str("                acc := xor(acc, add(acc, 7))\n");
            }
        }

        if self.has_binary_literal {
            source.push_str("                acc := xor(acc, 0xa3)\n");
        }
        if self.has_hex_literal {
            source.push_str("                acc := xor(acc, 0xfeed)\n");
        }
    }

    fn render_yul_helper_defs(&self, source: &mut String) {
        if self.has_high_level_operator || self.has_nested_helper_call {
            source.push_str(
                "            function yul_operator_mix(x, y) -> out {\n                let a := add(x, y)\n                let b := sub(a, and(y, 0xff))\n                let c := mul(b, 3)\n                let d := xor(or(c, y), and(x, 0xff))\n                let e := shr(1, shl(1, d))\n                let inv := not(x)\n                let bonus := 11\n                if lt(x, y) { bonus := add(bonus, 1) }\n                if gt(x, y) { bonus := add(bonus, 2) }\n                if eq(x, y) { bonus := add(bonus, 3) }\n                if eq(x, y) { bonus := add(bonus, 5) }\n                out := add(add(e, inv), bonus)\n            }\n",
            );
        }

        if self.has_core_ops_operator {
            source.push_str(
                "            function yul_core_operator_mix(x, y) -> out {\n                let safe_x := and(x, 0xffff)\n                let safe_y := and(y, 0xff)\n                let checked_sum := add(safe_x, safe_y)\n                let checked_diff := sub(checked_sum, safe_y)\n                let checked_mul := mul(checked_diff, 3)\n                let checked_mod := mod(checked_mul, 257)\n                let bonus := checked_mod\n                if iszero(gt(checked_diff, checked_sum)) { bonus := add(bonus, 0x31) }\n                if iszero(lt(checked_sum, checked_diff)) { bonus := add(bonus, 0x32) }\n                out := bonus\n            }\n",
            );
        }

        if self.has_nested_helper_call {
            source.push_str(
                "            function yul_nested_mix(x, y) -> out {\n                let t2 := add(x, y)\n                let tuple_mix := add(add(x, xor(y, 0x55)), t2)\n                let pair_mix := add(x, xor(y, 0x44))\n                out := yul_operator_mix(tuple_mix, pair_mix)\n            }\n",
            );
        }
    }

    fn classify(&self, classification: &mut SeedClassification) {
        classification.has_struct |= self.has_struct || self.has_comptime_control_flow;
        classification.has_tuple |= self.has_tuple;
        classification.has_compound_literal |= self.has_struct
            || self.has_tuple
            || self.has_parameterized_type
            || self.has_function_returns_compound
            || self.has_nested_compound
            || self.has_runtime_uninit;
        classification.has_field_access |= self.has_struct
            || self.has_tuple
            || self.has_parameterized_type
            || self.has_function_returns_compound
            || self.has_nested_compound
            || self.has_runtime_uninit;
        classification.has_field_update |=
            self.has_tuple || self.has_nested_compound || self.has_runtime_uninit;
        classification.has_comptime_type_reflection |= self.has_comptime_type_reflection;
        classification.has_cbytes_builtin |= self.has_cbytes_builtin;
        classification.has_high_level_operator |= self.has_high_level_operator;
        classification.has_core_ops_operator |= self.has_core_ops_operator;
        classification.has_helper_function |= self.has_helper_function;
        classification.has_nested_helper_call |= self.has_nested_helper_call;
        classification.has_import |= self.has_import;
        classification.has_import_single |= self.has_import_single;
        classification.has_import_group |= self.has_import;
        classification.has_import_alias |= self.has_import_alias;
        classification.has_import_glob |= self.has_import_glob;
        classification.has_deep_import |= self.has_deep_import;
        classification.has_comments |= self.has_comments;
        classification.has_binary_literal |= self.has_binary_literal;
        classification.has_hex_literal |= self.has_hex_literal;
        classification.has_parameterized_type |= self.has_parameterized_type;
        classification.has_comptime_control_flow |= self.has_comptime_control_flow;
        classification.has_comptime_loop |= self.has_comptime_loop;
        classification.has_type_dependent_branch |= self.has_type_dependent_branch;
        classification.has_function_returns_compound |= self.has_function_returns_compound;
        classification.has_function_early_return |= self.has_function_early_return;
        classification.has_nested_compound |= self.has_nested_compound;
        classification.has_runtime_uninit |= self.has_runtime_uninit;
        classification.has_data_offset |= self.has_data_offset;
        classification.has_std_registered |= self.has_core_ops_operator;
    }

    fn needs_compound_defs(&self) -> bool {
        self.has_struct
            || self.has_tuple
            || self.has_comptime_type_reflection
            || self.has_comptime_control_flow
            || self.has_helper_function
            || self.has_nested_helper_call
            || self.has_import
            || self.has_parameterized_type
            || self.has_type_dependent_branch
            || self.has_function_returns_compound
            || self.has_nested_compound
            || self.has_runtime_uninit
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

fn render_plank_entry(
    source: &mut String,
    entry_index: usize,
    entry: &Entry,
    mode: ProgramMode,
    frontend: &FrontendPlan,
) {
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
    frontend.render_plank_effects(source, entry_index);

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
                        "    let create_nonce_{fragment_index} = @evm_sload({CREATE2_NONCE_SLOT});"
                    )
                    .expect("writing to a string cannot fail");
                    writeln!(
                        source,
                        "    @evm_sstore({CREATE2_NONCE_SLOT}, @evm_add(create_nonce_{fragment_index}, 1));"
                    )
                    .expect("writing to a string cannot fail");
                    writeln!(
                        source,
                        "    let created_{fragment_index} = @evm_create2(0, {ptr}, {CREATE_INIT_BYTES}, @evm_xor({}, create_nonce_{fragment_index}));",
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

fn render_yul_entry(
    source: &mut String,
    entry_index: usize,
    entry: &Entry,
    mode: ProgramMode,
    frontend: &FrontendPlan,
) {
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
    frontend.render_yul_effects(source, entry_index);

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
                        "                let create_nonce_{fragment_index} := sload({CREATE2_NONCE_SLOT})"
                    )
                    .expect("writing to a string cannot fail");
                    writeln!(
                        source,
                        "                sstore({CREATE2_NONCE_SLOT}, add(create_nonce_{fragment_index}, 1))"
                    )
                    .expect("writing to a string cannot fail");
                    writeln!(
                        source,
                        "                let created_{fragment_index} := create2(0, {ptr}, {CREATE_INIT_BYTES}, xor({}, create_nonce_{fragment_index}))",
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
    use super::{
        CallStep, ConstantPool, CreateKind, Entry, EntryConfig, ExitConfig, ExitKind, Fragment,
        FrontendPlan, GeneratedCase, ProgramMode,
    };
    use arbitrary::{Arbitrary, Unstructured};
    use plank_driver::BackendKind;

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

    #[test]
    fn create2_rendering_uses_nonce_backed_salt() {
        let case = GeneratedCase {
            mode: ProgramMode::RawFallback,
            entries: vec![Entry {
                selector: 0,
                config: EntryConfig {
                    fragments: vec![Fragment::Create { kind: CreateKind::Create2, salt: 4 }],
                    exit: ExitConfig { kind: ExitKind::Stop, output_len: 0 },
                    constants: ConstantPool { words: [[0; 32]; 4] },
                },
            }],
            calls: vec![CallStep { selected_entry: 0, payload: Vec::new() }],
            frontend: Default::default(),
        };

        let plank = case.plank_source();
        let solidity = case.solidity_source();

        assert!(plank.contains("let create_nonce_0 = @evm_sload(0x02000000);"));
        assert!(plank.contains("@evm_sstore(0x02000000, @evm_add(create_nonce_0, 1));"));
        assert!(plank.contains(
            "let created_0 = @evm_create2(0, scratch +% 960, 13, @evm_xor(0x0, create_nonce_0));"
        ));

        assert!(solidity.contains("let create_nonce_0 := sload(0x02000000)"));
        assert!(solidity.contains("sstore(0x02000000, add(create_nonce_0, 1))"));
        assert!(solidity.contains(
            "let created_0 := create2(0, add(scratch, 960), 13, xor(0x0, create_nonce_0))"
        ));
    }

    #[test]
    fn frontend_feature_case_uses_source_set_imports_and_std() {
        let case = frontend_feature_case();
        let sources = case.plank_sources();

        assert_eq!(sources.files.len(), 6);
        assert_eq!(sources.std_mode, crate::StdMode::RepoStd);
        assert!(sources.to_string().contains("== gen/types.plk =="));
        assert!(sources.to_string().contains("== gen/extras.plk =="));
        assert!(sources.to_string().contains("== gen/glob_helpers.plk =="));
        assert!(sources.to_string().contains("== gen/nested/boxes.plk =="));
        assert!(sources.to_string().contains("import gen::types"));
        assert!(sources.to_string().contains("import gen::extras::SINGLE_IMPORT_MARKER;"));
        assert!(sources.to_string().contains("import gen::glob_helpers::*;"));
    }

    #[test]
    fn frontend_feature_case_compiles_plank() {
        let case = frontend_feature_case();

        crate::compiler::plank::compile_plank_sources(
            &case.plank_sources(),
            BackendKind::SirDebug,
            None,
        )
        .unwrap_or_else(|err| {
            panic!("feature case did not compile:\n{err}\n\n{}", case.plank_sources())
        });
    }

    #[test]
    fn frontend_feature_case_renders_new_compilation_features() {
        let case = frontend_feature_case();
        let sources = case.plank_sources().to_string();

        assert!(sources.contains("const Box = fn (comptime T: type) type"));
        assert!(sources.contains("@has_parameterized_name(BoxU256)"));
        assert!(
            sources.contains("import gen::extras::ALIAS_IMPORT_MARKER as ALIASED_IMPORT_MARKER;")
        );
        assert!(sources.contains("import gen::glob_helpers::*;"));
        assert!(sources.contains("import gen::nested::boxes::{DeepBoxU256 as ImportedBoxU256"));
        assert!(sources.contains("const FRONT_COMPTIME_LOOP_VALUE = comptime"));
        assert!(sources.contains("const front_type_score = fn (comptime T: type) u256"));
        assert!(sources.contains("const choose_front_value = fn"));
        assert!(sources.contains("const make_front_pair = fn"));
        assert!(sources.contains("const NestedOuter = struct"));
        assert!(sources.contains("let front_uninit_0 = @uninit(RuntimeScratch);"));
        assert!(sources.contains("@data_offset(@slice_cbytes"));
    }

    #[test]
    fn frontend_feature_case_classifies_new_compilation_features() {
        let classification = frontend_feature_case().seed_classification();

        assert!(classification.has_parameterized_type);
        assert!(classification.has_import_single);
        assert!(classification.has_import_group);
        assert!(classification.has_import_alias);
        assert!(classification.has_import_glob);
        assert!(classification.has_deep_import);
        assert!(classification.has_comptime_control_flow);
        assert!(classification.has_comptime_loop);
        assert!(classification.has_type_dependent_branch);
        assert!(classification.has_function_returns_compound);
        assert!(classification.has_function_early_return);
        assert!(classification.has_nested_compound);
        assert!(classification.has_runtime_uninit);
        assert!(classification.has_data_offset);
    }

    #[test]
    fn imported_frontend_symbols_have_fallback_definitions() {
        let case = GeneratedCase {
            mode: ProgramMode::RawFallback,
            entries: vec![Entry {
                selector: 0,
                config: EntryConfig {
                    fragments: Vec::new(),
                    exit: ExitConfig { kind: ExitKind::Stop, output_len: 0 },
                    constants: ConstantPool { words: [[0; 32]; 4] },
                },
            }],
            calls: vec![CallStep { selected_entry: 0, payload: Vec::new() }],
            frontend: FrontendPlan {
                has_struct: true,
                has_tuple: true,
                has_comptime_type_reflection: false,
                has_cbytes_builtin: true,
                has_high_level_operator: false,
                has_core_ops_operator: true,
                has_helper_function: true,
                has_nested_helper_call: false,
                has_import: true,
                has_comments: false,
                has_binary_literal: false,
                has_hex_literal: true,
                ..Default::default()
            },
        };
        let sources = case.plank_sources();
        let rendered = sources.to_string();

        assert!(rendered.contains("const FRONT_BINARY_LITERAL = 0;"));
        assert!(rendered.contains("const operator_mix = fn (x: u256, y: u256) u256"));
        crate::compiler::plank::compile_plank_sources(&sources, BackendKind::SirDebug, None)
            .unwrap_or_else(|err| {
                panic!("frontend fallback case did not compile:\n{err}\n\n{sources}")
            });
    }

    #[test]
    fn comptime_control_flow_renders_required_struct_definitions() {
        let case = GeneratedCase {
            mode: ProgramMode::RawFallback,
            entries: vec![Entry {
                selector: 0,
                config: EntryConfig {
                    fragments: Vec::new(),
                    exit: ExitConfig { kind: ExitKind::Stop, output_len: 0 },
                    constants: ConstantPool { words: [[0; 32]; 4] },
                },
            }],
            calls: vec![CallStep { selected_entry: 0, payload: Vec::new() }],
            frontend: FrontendPlan { has_comptime_control_flow: true, ..Default::default() },
        };
        let sources = case.plank_sources();
        let rendered = sources.to_string();

        assert!(rendered.contains("const Pair = struct"));
        assert!(rendered.contains("if @is_struct(Pair)"));
        crate::compiler::plank::compile_plank_sources(&sources, BackendKind::SirDebug, None)
            .unwrap_or_else(|err| {
                panic!("comptime control-flow case did not compile:\n{err}\n\n{sources}")
            });
    }

    #[test]
    fn helper_only_core_operator_effect_is_mirrored_in_yul() {
        let case = GeneratedCase {
            mode: ProgramMode::RawFallback,
            entries: vec![Entry {
                selector: 0,
                config: EntryConfig {
                    fragments: Vec::new(),
                    exit: ExitConfig { kind: ExitKind::Stop, output_len: 0 },
                    constants: ConstantPool { words: [[0; 32]; 4] },
                },
            }],
            calls: vec![CallStep { selected_entry: 0, payload: Vec::new() }],
            frontend: FrontendPlan {
                has_struct: false,
                has_tuple: false,
                has_comptime_type_reflection: false,
                has_cbytes_builtin: false,
                has_high_level_operator: false,
                has_core_ops_operator: true,
                has_helper_function: true,
                has_nested_helper_call: false,
                has_import: false,
                has_comments: false,
                has_binary_literal: false,
                has_hex_literal: false,
                ..Default::default()
            },
        };

        assert!(case.plank_source().contains("core_operator_mix(acc, 7)"));
        assert!(case.solidity_source().contains("yul_core_operator_mix(acc, 7)"));
    }

    #[test]
    #[ignore = "requires RAPPIE_SOL_SOLX, SOLX_PATH, or solx on PATH"]
    fn frontend_feature_case_compares_plank_solidity() {
        let case = frontend_feature_case();
        crate::compare_source_set(
            &case.plank_sources(),
            &case.solidity_source(),
            &case.calldatas(),
        )
        .unwrap_or_else(|err| {
            panic!(
                "feature case did not compare: {err}\n\nPlank:\n{}\nSolidity:\n{}",
                case.plank_sources(),
                case.solidity_source()
            )
        });
    }

    fn frontend_feature_case() -> GeneratedCase {
        GeneratedCase {
            mode: ProgramMode::RawFallback,
            entries: vec![Entry {
                selector: 0,
                config: EntryConfig {
                    fragments: Vec::new(),
                    exit: ExitConfig { kind: ExitKind::Stop, output_len: 0 },
                    constants: ConstantPool { words: [[0; 32]; 4] },
                },
            }],
            calls: vec![CallStep { selected_entry: 0, payload: Vec::new() }],
            frontend: FrontendPlan {
                has_struct: true,
                has_tuple: true,
                has_comptime_type_reflection: true,
                has_cbytes_builtin: true,
                has_high_level_operator: true,
                has_core_ops_operator: true,
                has_helper_function: true,
                has_nested_helper_call: true,
                has_import: true,
                has_comments: true,
                has_binary_literal: true,
                has_hex_literal: true,
                has_import_single: true,
                has_import_alias: true,
                has_import_glob: true,
                has_deep_import: true,
                has_parameterized_type: true,
                has_comptime_control_flow: true,
                has_comptime_loop: true,
                has_type_dependent_branch: true,
                has_function_returns_compound: true,
                has_function_early_return: true,
                has_nested_compound: true,
                has_runtime_uninit: true,
                has_data_offset: true,
            },
        }
    }
}
