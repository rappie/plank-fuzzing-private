# Repository Guidelines

## Project Structure & Module Organization

This monorepo contains the Plank compiler, standard library, docs, and editor tooling.
Key paths:

- `std/`: Plank standard library sources.
- `plankc/`: Rust compiler workspace. Core crates live in `crates/`, frontend crates in `frontend/`, and Sensei IR backend crates in `sir/crates/`.
- `plankc/plank-diff-tests/`: Plank/Solidity differential tests and examples using Foundry.
- `plankc/sir/sir-solidity-diff-tests/`: SIR/Solidity backend differential tests.
- `plank-doc/`: mdBook documentation.
- `plank-tree-sitter/`, `plank-vscode/`, `plank-zed/`: syntax grammar and editor integrations.

## Build, Test, and Development Commands

Run compiler commands from `plankc/` unless noted.

- `just build-debug`: build the `plank` compiler in debug mode.
- `just link-dev-local`: link the debug compiler and `std/` into `~/.plank` for local use.
- `just fmt`: format Rust and Foundry test files.
- `just check`: format, then run workspace Clippy with warnings denied.
- `just test`: run Rust tests with `cargo nextest`.
- `just test-all`: run Rust tests, SIR diff tests, Plank diff tests, and bytecode checks.
- `just ci-all`: full local pre-review check: tests, Clippy, and formatting checks.
- `just docs-serve`: serve `plank-doc/` locally with mdBook.

For tree-sitter work, run `npm test` in `plank-tree-sitter/`.

## Coding Style & Naming Conventions

Rust code uses edition 2024 and `cargo +nightly fmt --all`; keep lines near the configured 100-column width. Workspace lints deny unused crate dependencies and unreachable patterns. Prefer existing crate boundaries and naming patterns: crates use `plank-*` or `sir-*`, Rust modules use `snake_case`, and public types use `UpperCamelCase`. Plank test files use `.plk`; Solidity comparison tests use `.sol` and Foundry `.t.sol` names.

## Testing Guidelines

Add focused Rust unit or integration tests near changed crates, then run `just test`. For compiler output, backend, ABI, or EVM behavior changes, add or update cases under `plankc/plank-diff-tests/` or `plankc/sir/sir-solidity-diff-tests/` and run the relevant `just test-plank-diff` or `just test-sir-diff` target. Update bytecode snapshots only when the behavior change is intentional.

## Commit & Pull Request Guidelines

Recent history uses short imperative subjects, often with an emoji prefix and PR number, for example `🐛 fix duplicate ConstDef emitted into HIR (#247)`. Keep commits narrow and descriptive.

Before requesting review, run `just ci-all` from `plankc/`. PRs should describe the change, link related issues, call out intentional snapshot or bytecode changes, and disclose any LLM/AI assistance as required by `CONTRIBUTING.md`.
