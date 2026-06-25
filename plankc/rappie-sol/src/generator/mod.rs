use crate::evm::{HELPER_ECHO_WORD, HELPER_REVERT_WORD};
use alloy_primitives::U256;
use arbitrary::{Arbitrary, Unstructured};
use std::fmt::Write;

const MAX_DISPATCH_ENTRIES: usize = 4;
const MIN_INPUT_WORDS: usize = 1;
const MAX_INPUT_WORDS: usize = 4;
const MAX_RETURN_WORDS: usize = 4;
const MAX_DYNAMIC_BYTES: usize = 160;
const MAX_LOOP_ITERATIONS: usize = 8;
const MAX_MEMORY_SLOTS: usize = 6;
const MAX_STORAGE_SLOTS: usize = 3;
const MAX_LOG_WORDS: usize = 4;
const SCRATCH_BYTES: usize = 1024;
const CALL_INPUT_OFFSET: usize = 512;
const CALL_OUTPUT_OFFSET: usize = 544;
const LOOP_MEMORY_OFFSET: usize = 608;

const U256_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const I256_MIN: &str = "0x8000000000000000000000000000000000000000000000000000000000000000";
const I256_MAX: &str = "0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedCase {
    mode: ProgramMode,
    selected_entry: usize,
    entries: Vec<Entry>,
    calldata_words: Vec<u64>,
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

    pub(crate) fn calldata(&self) -> Vec<u8> {
        let selected = &self.entries[self.selected_entry];
        let mut calldata = Vec::with_capacity(
            self.calldata_words.len() * 32
                + if self.mode == ProgramMode::SelectorDispatch { 4 } else { 0 },
        );

        if self.mode == ProgramMode::SelectorDispatch {
            calldata.extend_from_slice(&selected.selector.to_be_bytes());
        }

        for &word in &self.calldata_words {
            calldata.extend_from_slice(&U256::from(word).to_be_bytes::<32>());
        }

        calldata
    }

    #[cfg(test)]
    fn selected_entry(&self) -> &Entry {
        &self.entries[self.selected_entry]
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

        let selected_entry = u.int_in_range(0..=entry_count - 1)?;
        let calldata_words = (0..entries[selected_entry].input_words)
            .map(|_| u64::arbitrary(u))
            .collect::<arbitrary::Result<Vec<_>>>()?;

        Ok(Self { mode, selected_entry, entries, calldata_words })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramMode {
    RawFallback,
    SelectorDispatch,
}

impl ProgramMode {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(if u.int_in_range(0..=2)? == 0 { Self::RawFallback } else { Self::SelectorDispatch })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    selector: u32,
    input_words: usize,
    config: EntryConfig,
}

impl Entry {
    fn arbitrary(u: &mut Unstructured<'_>, selector: u32) -> arbitrary::Result<Self> {
        Ok(Self {
            selector,
            input_words: u.int_in_range(MIN_INPUT_WORDS..=MAX_INPUT_WORDS)?,
            config: EntryConfig::arbitrary(u)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryConfig {
    return_words: usize,
    dynamic_len: usize,
    loop_iterations: usize,
    memory_slots: usize,
    storage_slots: usize,
    log_topics: usize,
    log_words: usize,
    call_kind: CallKind,
    exit_kind: ExitKind,
    constants: [u64; 4],
    byte_index: usize,
    shift: usize,
    salt: u32,
}

impl EntryConfig {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(Self {
            return_words: u.int_in_range(1..=MAX_RETURN_WORDS)?,
            dynamic_len: u.int_in_range(0..=MAX_DYNAMIC_BYTES)?,
            loop_iterations: u.int_in_range(0..=MAX_LOOP_ITERATIONS)?,
            memory_slots: u.int_in_range(1..=MAX_MEMORY_SLOTS)?,
            storage_slots: u.int_in_range(1..=MAX_STORAGE_SLOTS)?,
            log_topics: u.int_in_range(0..=4)?,
            log_words: u.int_in_range(0..=MAX_LOG_WORDS)?,
            call_kind: CallKind::arbitrary(u)?,
            exit_kind: ExitKind::arbitrary(u)?,
            constants: [
                u64::arbitrary(u)?,
                u64::arbitrary(u)?,
                u64::arbitrary(u)?,
                u64::arbitrary(u)?,
            ],
            byte_index: u.int_in_range(0..=31)?,
            shift: u.int_in_range(0..=255)?,
            salt: u32::arbitrary(u)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    EchoCall,
    EchoStaticCall,
    RevertCall,
}

impl CallKind {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=2)? {
            0 => Self::EchoCall,
            1 => Self::EchoStaticCall,
            _ => Self::RevertCall,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    ReturnWords,
    ReturnBytes,
    RevertWords,
    RevertBytes,
    Conditional,
}

impl ExitKind {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(match u.int_in_range(0..=4)? {
            0 => Self::ReturnWords,
            1 => Self::ReturnBytes,
            2 => Self::RevertWords,
            3 => Self::RevertBytes,
            _ => Self::Conditional,
        })
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

    let cfg = &entry.config;
    writeln!(source, "    let scratch = @malloc_zeroed({SCRATCH_BYTES});")
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "    let mut acc = @evm_xor(@evm_calldataload({}), {});",
        calldata_offset(mode, 0),
        edge_constant(cfg, 0)
    )
    .expect("writing to a string cannot fail");

    for input in 1..entry.input_words {
        let offset = calldata_offset(mode, input);
        writeln!(source, "    acc = @evm_add(acc, @evm_calldataload({offset}));")
            .expect("writing to a string cannot fail");
        writeln!(
            source,
            "    acc = @evm_xor(acc, @evm_shl({}, @evm_calldataload({offset})));",
            cfg.shift
        )
        .expect("writing to a string cannot fail");
    }

    render_plank_constants(source, cfg);
    render_plank_environment(source);
    render_plank_memory(source, cfg);
    render_plank_loop(source, cfg);
    render_plank_storage(source, cfg, entry_index);
    render_plank_call(source, cfg);
    render_plank_signed_ops(source, cfg);
    render_plank_log(source, cfg);
    render_plank_exit(source, cfg);

    source.push_str("};\n");
}

fn render_plank_constants(source: &mut String, cfg: &EntryConfig) {
    writeln!(source, "    acc = @evm_xor(acc, 0x0);").expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_add(acc, 0x1);").expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_xor(acc, 0xff);").expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_add(acc, 0xffff);").expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_xor(acc, {I256_MIN});")
        .expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_xor(acc, {I256_MAX});")
        .expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_addmod(acc, {}, {U256_MAX});", edge_constant(cfg, 1))
        .expect("writing to a string cannot fail");
    writeln!(source, "    acc = @evm_mulmod(acc, {}, {U256_MAX});", edge_constant(cfg, 2))
        .expect("writing to a string cannot fail");
}

fn render_plank_environment(source: &mut String) {
    for op in [
        "@evm_address_this()",
        "@evm_caller()",
        "@evm_callvalue()",
        "@evm_chainid()",
        "@evm_timestamp()",
        "@evm_number()",
        "@evm_basefee()",
    ] {
        writeln!(source, "    acc = @evm_xor(acc, {op});")
            .expect("writing to a string cannot fail");
    }
}

fn render_plank_memory(source: &mut String, cfg: &EntryConfig) {
    for slot in 0..cfg.memory_slots {
        let offset = slot * 32;
        let ptr = plank_ptr("scratch", offset);
        writeln!(source, "    @mstore32({ptr}, @evm_add(acc, {}));", edge_constant(cfg, slot))
            .expect("writing to a string cannot fail");
        writeln!(source, "    acc = @evm_xor(acc, @mload32({ptr}));")
            .expect("writing to a string cannot fail");
    }

    source.push_str("    acc = @evm_xor(acc, @evm_keccak256(scratch, 64));\n");
}

fn render_plank_loop(source: &mut String, cfg: &EntryConfig) {
    source.push_str("    let mut i = 0;\n");
    writeln!(source, "    while @evm_lt(i, {}) {{", cfg.loop_iterations)
        .expect("writing to a string cannot fail");
    writeln!(source, "        acc = @evm_add(acc, @evm_xor(i, {}));", edge_constant(cfg, 3))
        .expect("writing to a string cannot fail");
    writeln!(source, "        @mstore32({}, acc);", plank_ptr("scratch", LOOP_MEMORY_OFFSET))
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

fn render_plank_storage(source: &mut String, cfg: &EntryConfig, entry_index: usize) {
    for slot in 0..cfg.storage_slots {
        let storage_slot = storage_slot(entry_index, slot, cfg);
        writeln!(
            source,
            "    @evm_sstore({}, @evm_xor(acc, {}));",
            hex_u64(storage_slot),
            edge_constant(cfg, slot + 1)
        )
        .expect("writing to a string cannot fail");
        writeln!(source, "    acc = @evm_add(acc, @evm_sload({}));", hex_u64(storage_slot))
            .expect("writing to a string cannot fail");
    }
}

fn render_plank_call(source: &mut String, cfg: &EntryConfig) {
    writeln!(source, "    @mstore32({}, acc);", plank_ptr("scratch", CALL_INPUT_OFFSET))
        .expect("writing to a string cannot fail");

    let call = match cfg.call_kind {
        CallKind::EchoCall => format!(
            "@evm_call(100000, {HELPER_ECHO_WORD}, 0, {}, 32, {}, 32)",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::EchoStaticCall => format!(
            "@evm_staticcall(100000, {HELPER_ECHO_WORD}, {}, 32, {}, 32)",
            plank_ptr("scratch", CALL_INPUT_OFFSET),
            plank_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::RevertCall => format!(
            "@evm_call(100000, {HELPER_REVERT_WORD}, 0, {}, 32, {}, 32)",
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
}

fn render_plank_signed_ops(source: &mut String, cfg: &EntryConfig) {
    writeln!(source, "    if @evm_slt(acc, {I256_MIN}) {{")
        .expect("writing to a string cannot fail");
    writeln!(source, "        acc = @evm_xor(acc, @evm_sar({}, acc));", cfg.shift)
        .expect("writing to a string cannot fail");
    source.push_str("    } else {\n");
    writeln!(source, "        acc = @evm_add(acc, @evm_byte({}, acc));", cfg.byte_index)
        .expect("writing to a string cannot fail");
    source.push_str("    }\n");

    writeln!(source, "    if @evm_sgt(acc, {I256_MAX}) {{")
        .expect("writing to a string cannot fail");
    writeln!(source, "        acc = @evm_xor(acc, @evm_shr({}, acc));", cfg.shift)
        .expect("writing to a string cannot fail");
    source.push_str("    } else {\n");
    writeln!(source, "        acc = @evm_add(acc, @evm_shl({}, 1));", cfg.shift % 64)
        .expect("writing to a string cannot fail");
    source.push_str("    }\n");
}

fn render_plank_log(source: &mut String, cfg: &EntryConfig) {
    for word in 0..cfg.log_words {
        writeln!(
            source,
            "    @mstore32({}, @evm_add(acc, {}));",
            plank_ptr("scratch", word * 32),
            edge_constant(cfg, word)
        )
        .expect("writing to a string cannot fail");
    }

    let data_len = cfg.log_words * 32;
    let mut args = vec!["scratch".to_string(), data_len.to_string()];
    args.extend((0..cfg.log_topics).map(|topic| plank_topic_expr(cfg, topic)));
    writeln!(source, "    @evm_log{}({});", cfg.log_topics, args.join(", "))
        .expect("writing to a string cannot fail");
}

fn render_plank_exit(source: &mut String, cfg: &EntryConfig) {
    match cfg.exit_kind {
        ExitKind::ReturnWords => {
            render_plank_output_words(source, cfg.return_words, cfg);
            writeln!(source, "    @evm_return(scratch, {});", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
        }
        ExitKind::ReturnBytes => {
            render_plank_output_words(source, words_for_len(cfg.dynamic_len), cfg);
            writeln!(source, "    @evm_return(scratch, {});", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::RevertWords => {
            render_plank_output_words(source, cfg.return_words, cfg);
            writeln!(source, "    @evm_revert(scratch, {});", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
        }
        ExitKind::RevertBytes => {
            render_plank_output_words(source, words_for_len(cfg.dynamic_len), cfg);
            writeln!(source, "    @evm_revert(scratch, {});", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::Conditional => {
            source.push_str("    if @evm_lt(@evm_and(acc, 0xff), 0x80) {\n");
            render_plank_output_words_with_indent(source, cfg.return_words, cfg, 2);
            writeln!(source, "        @evm_return(scratch, {});", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
            source.push_str("    } else {\n");
            render_plank_output_words_with_indent(source, words_for_len(cfg.dynamic_len), cfg, 2);
            writeln!(source, "        @evm_revert(scratch, {});", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
            source.push_str("    }\n");
        }
    }
}

fn render_plank_output_words(source: &mut String, words: usize, cfg: &EntryConfig) {
    render_plank_output_words_with_indent(source, words, cfg, 1);
}

fn render_plank_output_words_with_indent(
    source: &mut String,
    words: usize,
    cfg: &EntryConfig,
    indent: usize,
) {
    let indent = "    ".repeat(indent);
    for word in 0..words {
        writeln!(
            source,
            "{indent}@mstore32({}, @evm_add(acc, {}));",
            plank_ptr("scratch", word * 32),
            edge_constant(cfg, word + 2)
        )
        .expect("writing to a string cannot fail");
    }
}

fn render_yul_entry(source: &mut String, entry_index: usize, entry: &Entry, mode: ProgramMode) {
    writeln!(source, "            function entry_{entry_index}() {{")
        .expect("writing to a string cannot fail");

    let cfg = &entry.config;
    writeln!(source, "                let scratch := mload(0x40)")
        .expect("writing to a string cannot fail");
    writeln!(source, "                mstore(0x40, add(scratch, {SCRATCH_BYTES}))")
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "                let acc := xor(calldataload({}), {})",
        calldata_offset(mode, 0),
        edge_constant(cfg, 0)
    )
    .expect("writing to a string cannot fail");

    for input in 1..entry.input_words {
        let offset = calldata_offset(mode, input);
        writeln!(source, "                acc := add(acc, calldataload({offset}))")
            .expect("writing to a string cannot fail");
        writeln!(
            source,
            "                acc := xor(acc, shl({}, calldataload({offset})))",
            cfg.shift
        )
        .expect("writing to a string cannot fail");
    }

    render_yul_constants(source, cfg);
    render_yul_environment(source);
    render_yul_memory(source, cfg);
    render_yul_loop(source, cfg);
    render_yul_storage(source, cfg, entry_index);
    render_yul_call(source, cfg);
    render_yul_signed_ops(source, cfg);
    render_yul_log(source, cfg);
    render_yul_exit(source, cfg);

    source.push_str("            }\n");
}

fn render_yul_constants(source: &mut String, cfg: &EntryConfig) {
    source.push_str("                acc := xor(acc, 0x0)\n");
    source.push_str("                acc := add(acc, 0x1)\n");
    source.push_str("                acc := xor(acc, 0xff)\n");
    source.push_str("                acc := add(acc, 0xffff)\n");
    writeln!(source, "                acc := xor(acc, {I256_MIN})")
        .expect("writing to a string cannot fail");
    writeln!(source, "                acc := xor(acc, {I256_MAX})")
        .expect("writing to a string cannot fail");
    writeln!(source, "                acc := addmod(acc, {}, {U256_MAX})", edge_constant(cfg, 1))
        .expect("writing to a string cannot fail");
    writeln!(source, "                acc := mulmod(acc, {}, {U256_MAX})", edge_constant(cfg, 2))
        .expect("writing to a string cannot fail");
}

fn render_yul_environment(source: &mut String) {
    for op in [
        "address()",
        "caller()",
        "callvalue()",
        "chainid()",
        "timestamp()",
        "number()",
        "basefee()",
    ] {
        writeln!(source, "                acc := xor(acc, {op})")
            .expect("writing to a string cannot fail");
    }
}

fn render_yul_memory(source: &mut String, cfg: &EntryConfig) {
    for slot in 0..cfg.memory_slots {
        let offset = slot * 32;
        let ptr = yul_ptr("scratch", offset);
        writeln!(source, "                mstore({ptr}, add(acc, {}))", edge_constant(cfg, slot))
            .expect("writing to a string cannot fail");
        writeln!(source, "                acc := xor(acc, mload({ptr}))")
            .expect("writing to a string cannot fail");
    }

    source.push_str("                acc := xor(acc, keccak256(scratch, 64))\n");
}

fn render_yul_loop(source: &mut String, cfg: &EntryConfig) {
    writeln!(
        source,
        "                for {{ let i := 0 }} lt(i, {}) {{ i := add(i, 1) }} {{",
        cfg.loop_iterations
    )
    .expect("writing to a string cannot fail");
    writeln!(source, "                    acc := add(acc, xor(i, {}))", edge_constant(cfg, 3))
        .expect("writing to a string cannot fail");
    writeln!(source, "                    mstore({}, acc)", yul_ptr("scratch", LOOP_MEMORY_OFFSET))
        .expect("writing to a string cannot fail");
    writeln!(
        source,
        "                    acc := xor(acc, mload({}))",
        yul_ptr("scratch", LOOP_MEMORY_OFFSET)
    )
    .expect("writing to a string cannot fail");
    source.push_str("                }\n");
}

fn render_yul_storage(source: &mut String, cfg: &EntryConfig, entry_index: usize) {
    for slot in 0..cfg.storage_slots {
        let storage_slot = storage_slot(entry_index, slot, cfg);
        writeln!(
            source,
            "                sstore({}, xor(acc, {}))",
            hex_u64(storage_slot),
            edge_constant(cfg, slot + 1)
        )
        .expect("writing to a string cannot fail");
        writeln!(source, "                acc := add(acc, sload({}))", hex_u64(storage_slot))
            .expect("writing to a string cannot fail");
    }
}

fn render_yul_call(source: &mut String, cfg: &EntryConfig) {
    writeln!(source, "                mstore({}, acc)", yul_ptr("scratch", CALL_INPUT_OFFSET))
        .expect("writing to a string cannot fail");

    let call = match cfg.call_kind {
        CallKind::EchoCall => format!(
            "call(100000, {HELPER_ECHO_WORD}, 0, {}, 32, {}, 32)",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::EchoStaticCall => format!(
            "staticcall(100000, {HELPER_ECHO_WORD}, {}, 32, {}, 32)",
            yul_ptr("scratch", CALL_INPUT_OFFSET),
            yul_ptr("scratch", CALL_OUTPUT_OFFSET)
        ),
        CallKind::RevertCall => format!(
            "call(100000, {HELPER_REVERT_WORD}, 0, {}, 32, {}, 32)",
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
}

fn render_yul_signed_ops(source: &mut String, cfg: &EntryConfig) {
    writeln!(source, "                switch slt(acc, {I256_MIN})")
        .expect("writing to a string cannot fail");
    writeln!(source, "                case 0 {{ acc := add(acc, byte({}, acc)) }}", cfg.byte_index)
        .expect("writing to a string cannot fail");
    writeln!(source, "                default {{ acc := xor(acc, sar({}, acc)) }}", cfg.shift)
        .expect("writing to a string cannot fail");

    writeln!(source, "                switch sgt(acc, {I256_MAX})")
        .expect("writing to a string cannot fail");
    writeln!(source, "                case 0 {{ acc := add(acc, shl({}, 1)) }}", cfg.shift % 64)
        .expect("writing to a string cannot fail");
    writeln!(source, "                default {{ acc := xor(acc, shr({}, acc)) }}", cfg.shift)
        .expect("writing to a string cannot fail");
}

fn render_yul_log(source: &mut String, cfg: &EntryConfig) {
    for word in 0..cfg.log_words {
        writeln!(
            source,
            "                mstore({}, add(acc, {}))",
            yul_ptr("scratch", word * 32),
            edge_constant(cfg, word)
        )
        .expect("writing to a string cannot fail");
    }

    let data_len = cfg.log_words * 32;
    let mut args = vec!["scratch".to_string(), data_len.to_string()];
    args.extend((0..cfg.log_topics).map(|topic| yul_topic_expr(cfg, topic)));
    writeln!(source, "                log{}({})", cfg.log_topics, args.join(", "))
        .expect("writing to a string cannot fail");
}

fn render_yul_exit(source: &mut String, cfg: &EntryConfig) {
    match cfg.exit_kind {
        ExitKind::ReturnWords => {
            render_yul_output_words(source, cfg.return_words, cfg);
            writeln!(source, "                return(scratch, {})", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
        }
        ExitKind::ReturnBytes => {
            render_yul_output_words(source, words_for_len(cfg.dynamic_len), cfg);
            writeln!(source, "                return(scratch, {})", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::RevertWords => {
            render_yul_output_words(source, cfg.return_words, cfg);
            writeln!(source, "                revert(scratch, {})", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
        }
        ExitKind::RevertBytes => {
            render_yul_output_words(source, words_for_len(cfg.dynamic_len), cfg);
            writeln!(source, "                revert(scratch, {})", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
        }
        ExitKind::Conditional => {
            source.push_str("                switch lt(and(acc, 0xff), 0x80)\n");
            source.push_str("                case 0 {\n");
            render_yul_output_words_with_indent(source, words_for_len(cfg.dynamic_len), cfg, 5);
            writeln!(source, "                    revert(scratch, {})", cfg.dynamic_len)
                .expect("writing to a string cannot fail");
            source.push_str("                }\n");
            source.push_str("                default {\n");
            render_yul_output_words_with_indent(source, cfg.return_words, cfg, 5);
            writeln!(source, "                    return(scratch, {})", cfg.return_words * 32)
                .expect("writing to a string cannot fail");
            source.push_str("                }\n");
        }
    }
}

fn render_yul_output_words(source: &mut String, words: usize, cfg: &EntryConfig) {
    render_yul_output_words_with_indent(source, words, cfg, 4);
}

fn render_yul_output_words_with_indent(
    source: &mut String,
    words: usize,
    cfg: &EntryConfig,
    indent: usize,
) {
    let indent = "    ".repeat(indent);
    for word in 0..words {
        writeln!(
            source,
            "{indent}mstore({}, add(acc, {}))",
            yul_ptr("scratch", word * 32),
            edge_constant(cfg, word + 2)
        )
        .expect("writing to a string cannot fail");
    }
}

fn plank_topic_expr(cfg: &EntryConfig, topic: usize) -> String {
    match topic {
        0 => "acc".to_string(),
        1 => format!("@evm_xor(acc, {})", edge_constant(cfg, 0)),
        2 => format!("@evm_add(acc, {})", edge_constant(cfg, 1)),
        _ => format!("@evm_xor(acc, {})", edge_constant(cfg, 2)),
    }
}

fn yul_topic_expr(cfg: &EntryConfig, topic: usize) -> String {
    match topic {
        0 => "acc".to_string(),
        1 => format!("xor(acc, {})", edge_constant(cfg, 0)),
        2 => format!("add(acc, {})", edge_constant(cfg, 1)),
        _ => format!("xor(acc, {})", edge_constant(cfg, 2)),
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
    0x0100_0000u64
        + ((entry_index as u64) << 16)
        + ((slot_index as u64) << 8)
        + u64::from(cfg.salt & 0xff)
}

fn edge_constant(cfg: &EntryConfig, index: usize) -> String {
    match index % 10 {
        0 => hex_u64(cfg.constants[0]),
        1 => hex_u64(cfg.constants[1]),
        2 => "0x0".to_string(),
        3 => "0x1".to_string(),
        4 => "0xff".to_string(),
        5 => "0xffff".to_string(),
        6 => I256_MAX.to_string(),
        7 => I256_MIN.to_string(),
        8 => U256_MAX.to_string(),
        _ => hex_u64(cfg.constants[2] ^ cfg.constants[3]),
    }
}

fn words_for_len(len: usize) -> usize {
    usize::max(1, len.div_ceil(32))
}

fn plank_ptr(base: &str, offset: usize) -> String {
    if offset == 0 { base.to_string() } else { format!("{base} +% {offset}") }
}

fn yul_ptr(base: &str, offset: usize) -> String {
    if offset == 0 { base.to_string() } else { format!("add({base}, {offset})") }
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

        assert!(plank.contains("@evm_sstore"));
        assert!(plank.contains("@evm_log"));
        assert!(plank.contains("@evm_call") || plank.contains("@evm_staticcall"));
        assert!(plank.contains("@evm_chainid"));
        assert!(plank.contains("while @evm_lt"));
        assert!(plank.contains("@evm_return") || plank.contains("@evm_revert"));

        assert!(solidity.contains("contract C {"));
        assert!(solidity.contains("sstore("));
        assert!(solidity.contains("log"));
        assert!(solidity.contains("call(") || solidity.contains("staticcall("));
        assert!(solidity.contains("chainid()"));
        assert!(solidity.contains("return(") || solidity.contains("revert("));
    }

    #[test]
    fn selector_dispatch_calldata_uses_selected_selector_prefix() {
        let bytes = vec![255; 4096];
        let mut u = Unstructured::new(&bytes);
        let case = GeneratedCase::arbitrary(&mut u).expect("case should decode");
        let calldata = case.calldata();

        if case.plank_source().contains("SELECTOR_") {
            assert_eq!(&calldata[..4], &case.selected_entry().selector.to_be_bytes());
            assert_eq!((calldata.len() - 4) % 32, 0);
        } else {
            assert_eq!(calldata.len() % 32, 0);
        }
    }
}
