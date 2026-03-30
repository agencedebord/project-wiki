# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-03-30

Consolidation release: stable memory core, real migration, reproducible demo.

### Added

- `context --file` surfaces top 3 prioritized memory items before editing (auto via Claude Code hook)
- `check-diff` flags affected decisions after changes (supports `--json`, `--staged`, `--pr-comment`)
- `generate-candidates` detects exceptions, decisions, business rules from code patterns
- `promote` / `reject` commands to curate candidates into confirmed memory items
- Shared prioritization logic (exception > decision > business_rule, then confidence, then related files)
- Migration status check (#11) in `validate` — reports notes without `memory_items`
- JSON shape tests for `context` and `check-diff` outputs
- Snapshot tests for text and PR comment formats
- 7 domain notes with 30 memory items (check-diff, context, validate, promote, drift, note, candidates)
- `.wiki/_artifacts.md` clarifying source of truth vs derived vs transitional files
- Reproducible demo fixture in `fixtures/` with `run-demo.sh`
- Demo scenarios in `examples/demo-v0.2.md`

### Changed

- README aligned with real product behavior (hooks, CLAUDE.md path, no overselling)
- Remodularized `check_diff`, `context`, `validate`, `candidates` into sub-modules with clear boundaries
- `.wiki/` tracked in git (excluding derived artifacts)

### Fixed

- Provenance parser in `promote` incorrectly matched top-level metadata as provenance entries
- All clippy warnings resolved (11 fixes across 8 files)
- Test re-coupling: `note.rs` no longer imports from `check_diff`/`context` in tests

## [0.1.0] - 2026-03-26

### Added

- 14 CLI commands: `init`, `status`, `validate`, `consult`, `graph`, `search`, `add domain`, `add context`, `add decision`, `rebuild`, `index`, `confirm`, `deprecate`, `rename-domain`, `import`
- 3-pass codebase scanner (structure, relations, details) supporting JS/TS, Python, Rust, Go
- Confidence system with 5 levels: `confirmed`, `verified`, `seen-in-code`, `inferred`, `needs-validation`
- Optional Notion import via `--features notion` with batch pagination, resume support, and contradiction detection
- Claude Code integration: auto-patches `.claude/CLAUDE.md` with wiki instructions
- Machine-readable `_index.json` for LLM consumption
- Configurable staleness thresholds via `.wiki/config.toml`
- `validate` command checking broken links, dead references, staleness, orphan notes, deprecated references
- Mermaid dependency graph generation
- Full-text search with Unicode-safe highlighting
- Path traversal sanitization on domain names
- Domain rename with automatic cross-reference updates
- External markdown import with front matter handling
- Beautiful terminal UI with progress bars and color output
- Property-based tests with proptest
- Dual license: MIT OR Apache-2.0

[0.2.0]: https://github.com/agencedebord/project-wiki/releases/tag/v0.2.0
[0.1.0]: https://github.com/agencedebord/project-wiki/releases/tag/v0.1.0
