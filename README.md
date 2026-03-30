# project-wiki

**Repo-native decisional memory for AI-assisted code changes. Prevents humans and AIs from breaking implicit rules.**

[![CI](https://github.com/agencedebord/project-wiki/actions/workflows/ci.yml/badge.svg)](https://github.com/agencedebord/project-wiki/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-blue)](https://www.rust-lang.org)

---

## The problem

Every project accumulates invisible knowledge: why deduplication is disabled, why that endpoint returns 200 instead of 201, why the billing module talks to the notification service. This knowledge lives in Notion tickets, Slack threads, PR comments, and the memories of people who might leave.

When an AI assistant -- or a new developer -- works on the codebase without this context, they "fix" intentional behavior. They undo decisions. They break things that looked broken but weren't.

**The cost is real**: a single misunderstood business rule can cause a regression that takes days to identify, because nothing in the code says "this was on purpose."

## The solution

`project-wiki` creates a `.wiki/` directory in your repo containing structured **memory items** -- exceptions, decisions, and business rules -- linked to the files they affect.

These memory items are surfaced at three critical moments:

| Moment | Command | What it does |
|--------|---------|-------------|
| **Before editing** | `context --file src/billing/invoice.ts` | Injects relevant memory items into the AI's context window |
| **After a diff** | `check-diff` | Surfaces exceptions and decisions that affect the modified files |
| **Continuous** | `detect-drift --file ...` | Flags when a tracked file changes and its wiki note is stale |

## Quick start

```bash
# Install from source
git clone https://github.com/agencedebord/project-wiki.git
cd project-wiki && cargo install --path .

# Initialize (scans your codebase to bootstrap domains)
cd your-project
project-wiki init

# See what was found
project-wiki status
```

## Memory items

The core unit of knowledge is a **memory item** in a domain note's YAML front matter:

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
    text: Le client X utilise encore l'ancien calcul
    confidence: confirmed
    related_files:
      - src/billing/legacy_pricing.ts
    sources:
      - kind: file
        ref: src/billing/legacy_pricing.ts
    status: active
  - id: billing-002
    type: decision
    text: Pas de deduplication des lignes importees
    confidence: verified
    sources:
      - kind: file
        ref: src/billing/import.ts
    status: active
---
```

Three types, ordered by danger level:

| Type | Meaning | Why it matters |
|------|---------|---------------|
| `exception` | A deviation from the norm | Most dangerous to ignore -- an AI will "fix" it |
| `decision` | An explicit architectural/business choice | Breaking it means undoing a deliberate tradeoff |
| `business_rule` | A known rule the code implements | Context for understanding behavior |

## Key commands

### Before editing: `context`

```bash
$ project-wiki context --file src/billing/invoice.ts

[project-wiki] Domain: billing (confidence: verified, updated: 2026-03-28)

Memory:
  [exception] Le client X utilise encore l'ancien calcul [confirmed]
  [decision] Pas de deduplication des lignes importees [verified]
  [business_rule] La facture est emise apres synchro [seen-in-code]

Dependencies: payments, taxes
```

Used automatically via Claude Code hooks (installed by `project-wiki init`).

### After a diff: `check-diff`

```bash
$ project-wiki check-diff src/billing/invoice.ts src/billing/service.ts

[project-wiki] Diff check

2 file(s) analyzed
1 domain(s) affected
Sensitivity: high

Affected domains
  billing (primary) — 2 file(s), 3 item(s)

Priority memory
  billing:
    [exception] Le client X utilise encore l'ancien calcul [confirmed] *
    [decision] Pas de deduplication des lignes importees [verified]
    [business_rule] La facture est emise apres synchro [seen-in-code]

Suggested actions
  → Verifier si l'exception 'Le client X utilise encore l'ancien calcul' reste valide
```

Also supports `--json` for programmatic use and `--staged` for pre-commit checks.

### Confirm knowledge: `confirm`

```bash
# Confirm a whole domain note
project-wiki confirm billing

# Confirm a single memory item by ID
project-wiki confirm billing-001
```

Confirming an item sets its confidence to `confirmed` and updates `last_reviewed`.

## Confidence system

Every note and every memory item carries a confidence level:

| Level | Trust it? |
|-------|-----------|
| `confirmed` | Yes -- validated by a human |
| `verified` | Yes -- cross-checked in code + docs |
| `seen-in-code` | Mostly -- verify if critical |
| `inferred` | Maybe -- check before relying on it |
| `needs-validation` | No -- verify first |

**Golden rule**: if the wiki contradicts the code, the code wins. Update the wiki.

## All commands

### Read

```bash
project-wiki consult [domain]    # Read notes for a domain (or --all)
project-wiki search <term>       # Full-text search across all notes
project-wiki status              # Wiki health: coverage, staleness, confidence
```

### Write

```bash
project-wiki add domain <name>           # Create a new domain
project-wiki add context "<text>"        # Add knowledge (auto-routed to domain)
project-wiki add decision "<text>"       # Record a business decision
project-wiki confirm <target>            # Promote confidence (domain or item ID)
project-wiki deprecate <target>          # Mark as deprecated
project-wiki rename-domain <old> <new>   # Rename + update all references
project-wiki import <folder> <domain>    # Import external markdown files
```

### Analyze

```bash
project-wiki check-diff [files]  # Check modified files against wiki memory
project-wiki detect-drift --file <path>  # Detect wiki drift for a file
project-wiki validate            # Check for broken links, dead refs, staleness
```

### Maintain

```bash
project-wiki rebuild             # Regenerate graph + index
project-wiki index               # Regenerate _index.md and _index.json
project-wiki graph               # Display the dependency graph
```

### Hooks

```bash
project-wiki install-hooks       # Install Claude Code hooks
project-wiki uninstall-hooks     # Remove Claude Code hooks
```

## Claude Code integration

`project-wiki init` automatically installs Claude Code hooks:

1. **PreToolUse** (`context --hook`): before any file edit, injects relevant memory items
2. **PostToolUse** (`detect-drift --hook`): after a file write, flags potential wiki drift

It also patches `.claude/CLAUDE.md` with instructions to read, respect, and update the wiki.

## Configuration

`.wiki/config.toml`:

```toml
# Days before a note is considered stale (default: 30)
staleness_days = 30

# Auto-regenerate _index.json after mutations (default: true)
auto_index = true
```

## Current state

This project is in active development. The 30-day roadmap (memory items, context v1, check-diff, confirm items) is implemented. The scan and graph features are functional but secondary to the core memory layer.

Supports **JavaScript/TypeScript**, **Python**, **Rust**, and **Go** project structures for domain detection.

## Installation

```bash
# From source
git clone https://github.com/agencedebord/project-wiki.git
cd project-wiki
cargo build --release
# Binary: target/release/project-wiki
```

Requires Rust 1.85.0+.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and guidelines.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.
