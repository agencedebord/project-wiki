# codefidence

**Structured memory items that surface context before you edit and check decisions after you diff.**

[![CI](https://github.com/agencedebord/codefidence/actions/workflows/ci.yml/badge.svg)](https://github.com/agencedebord/codefidence/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-blue)](https://www.rust-lang.org)

---

## The problem

Every project accumulates invisible knowledge: why deduplication is disabled, why that endpoint returns 200 instead of 201, why the billing module talks to the notification service. This knowledge lives in Notion tickets, Slack threads, PR comments, and the memories of people who might leave.

When an AI assistant -- or a new developer -- works on the codebase without this context, they "fix" intentional behavior. They undo decisions. They break things that looked broken but weren't.

## What codefidence does

`codefidence` creates a `.wiki/` directory in your repo containing structured **memory items** -- exceptions, decisions, and business rules -- linked to the files they affect.

These memory items are surfaced at two critical moments:

| Moment | Command | What happens |
|--------|---------|-------------|
| **Before editing** | `context --file src/billing/invoice.ts` | Surfaces relevant memory items so you (or your AI) know what to preserve |
| **After a diff** | `check-diff` | Flags exceptions and decisions that affect the modified files |

`context` runs automatically via a Claude Code hook before file edits. `check-diff` is run manually or in CI.

## Quick start

```bash
# Install from source (Rust 1.85+)
git clone https://github.com/agencedebord/codefidence.git
cd codefidence && cargo install --path .

# Initialize in your project
cd your-project
codefidence init --full    # scans codebase, installs hooks, patches CLAUDE.md

# See what was found
codefidence status
```

`init --full` does four things: scans your codebase to bootstrap domain notes, installs Claude Code hooks, patches `CLAUDE.md` (at project root) with wiki instructions, and installs Claude slash commands. Each step is also available separately (`--scan`, `--hooks`).

## Memory items

The core primitive is a **memory item** in a domain note's YAML front matter:

```yaml
---
title: Billing overview
confidence: verified
last_updated: "2026-03-28"
related_files:
  - src/billing/invoice.ts
  - src/billing/service.ts
memory_items:
  - id: billing-001
    type: exception
    text: Client X still uses the legacy pricing calculation
    confidence: confirmed
    related_files:
      - src/billing/legacy_pricing.ts
    sources:
      - kind: file
        ref: src/billing/legacy_pricing.ts
    status: active
  - id: billing-002
    type: decision
    text: No deduplication on imported rows
    confidence: verified
    sources:
      - kind: file
        ref: src/billing/import.ts
    status: active
---
```

Three types, ordered by danger level:

| Type | Meaning |
|------|---------|
| `exception` | A deviation from the norm -- most dangerous to ignore, an AI will "fix" it |
| `decision` | An explicit architectural or business choice -- breaking it undoes a deliberate tradeoff |
| `business_rule` | A known rule the code implements -- context for understanding behavior |

## Key commands

### `context` -- before editing

```bash
$ codefidence context --file src/billing/invoice.ts

[codefidence] Domain: billing (confidence: verified, updated: 2026-03-28)

Memory:
  [exception] Client X still uses the legacy pricing calculation [confirmed]
  [decision] No deduplication on imported rows [verified]
  [business_rule] Invoice is issued after sync completes [seen-in-code]

Dependencies: payments, taxes
```

When Claude Code hooks are installed, this runs automatically before file edits via the PreToolUse hook.

### `check-diff` -- after changes

```bash
$ codefidence check-diff src/billing/invoice.ts src/billing/service.ts

[codefidence] Diff check

2 file(s) analyzed
1 domain(s) affected
Sensitivity: high

Affected domains
  billing (primary) — 2 file(s), 3 item(s)

Priority memory
  billing:
    [exception] Client X still uses the legacy pricing calculation [confirmed] *
    [decision] No deduplication on imported rows [verified]

Suggested actions
  → Verify whether the exception 'Client X still uses the legacy pricing calculation' is still valid
```

With no arguments, checks unstaged git changes. Also supports `--staged`, `--json`, and `--pr-comment`.

### `confirm` -- validate knowledge

```bash
# Confirm a whole domain note
codefidence confirm billing

# Confirm a single memory item
codefidence confirm billing-001
```

## Confidence system

Every note and memory item carries a confidence level:

| Level | Trust it? |
|-------|-----------|
| `confirmed` | Yes -- validated by a human |
| `verified` | Yes -- cross-checked in code + docs |
| `seen-in-code` | Mostly -- verify if critical |
| `inferred` | Maybe -- check before relying on it |
| `needs-validation` | No -- verify first |

**Rule**: if the wiki contradicts the code, the code wins. Update the wiki.

## All commands

```
Read
  consult [domain]           Read notes for a domain (or --all)
  search <term>              Full-text search across all notes
  status                     Wiki health: coverage, staleness, confidence
  review [domain]            Interactive review of domain notes and items

Write
  add domain <name>          Create a new domain
  add context "<text>"       Add knowledge (auto-routed to domain)
  add decision "<text>"      Record a business decision
  confirm <target>           Promote confidence (domain or item ID)
  deprecate <target>         Mark as deprecated
  rename-domain <old> <new>  Rename + update all references
  import <folder>            Import external markdown files

Analyze
  context --file <path>      Surface memory items for a file
  check-diff [files]         Check modified files against wiki memory
  detect-drift --file <path> Flag when a tracked file's wiki note is stale
  validate                   Check for broken links, dead refs, staleness

Maintain
  rebuild                    Regenerate graph + index
  index                      Regenerate _index.md and _index.json
  graph                      Display the dependency graph

Candidates
  generate-candidates        Scan codebase for potential memory items
  promote <id>               Promote a candidate to confirmed memory item
  promote --next             Auto-promote the highest-priority pending candidate
  reject <id>                Reject a candidate

Hooks
  install-hooks              Install Claude Code hooks
  uninstall-hooks            Remove Claude Code hooks
```

## Claude Code integration

`codefidence init --full` installs two Claude Code hooks:

1. **PreToolUse** (`context --hook`): before file edits, injects relevant memory items into the AI's context
2. **PostToolUse** (`detect-drift --hook`): after a file write, flags potential wiki drift

It also patches `CLAUDE.md` (at project root) with instructions to read and respect the wiki.

## Configuration

`.wiki/config.toml`:

```toml
# Days before a note is considered stale (default: 30)
staleness_days = 30

# Auto-regenerate _index.json after mutations (default: true)
auto_index = true
```

## Current state (v0.2.0)

The core loop works: memory items are parsed, surfaced by `context`, and checked by `check-diff`. The `confirm` and `promote` commands let you curate knowledge over time.

The codebase scan detects project structure for TypeScript/JavaScript, Python, Rust, Go, Ruby, Java, and PHP. TypeScript is the most tested target. The tool itself is language-agnostic -- memory items are linked to file paths, not to language-specific constructs.

CI integration (`check-diff --pr-comment`) exists but is minimal: it outputs markdown suitable for a PR comment. There is no GitHub Action published yet.

This is pre-1.0 software in active development.

## Installation

```bash
git clone https://github.com/agencedebord/codefidence.git
cd codefidence
cargo install --path .
```

Requires Rust 1.85.0+.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and guidelines.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.
