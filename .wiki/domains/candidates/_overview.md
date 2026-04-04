---
title: Candidates overview
domain: candidates
confidence: seen-in-code
last_updated: "2026-04-04"
related_files:
  - src/init/candidates/heuristics.rs
memory_items:
  - id: candidates-001
    type: business_rule
    text: "Exception candidates are triggered by filename patterns: legacy|compat|override|workaround|migration|deprecated|old_|_old|_v1|v1_"
    confidence: seen-in-code
    related_files:
      - src/init/candidates/heuristics.rs
    sources:
      - kind: file
        ref: src/init/candidates/heuristics.rs
        line: 13
    status: active
    last_reviewed: "2026-03-30"
  - id: candidates-002
    type: decision
    text: "Minimum 50 lines required for candidate generation (MIN_FILE_LINES = 50), filtering out trivial files"
    confidence: seen-in-code
    related_files:
      - src/init/candidates/heuristics.rs
    sources:
      - kind: file
        ref: src/init/candidates/heuristics.rs
        line: 32
    status: active
    last_reviewed: "2026-03-30"
  - id: candidates-003
    type: business_rule
    text: "Decision candidates require a keyword in comments: decision|chosen|we decided|deliberately|intentionally|on purpose|trade-off|ADR"
    confidence: seen-in-code
    related_files:
      - src/init/candidates/heuristics.rs
    sources:
      - kind: file
        ref: src/init/candidates/heuristics.rs
        line: 18
    status: active
    last_reviewed: "2026-03-30"
  - id: candidates-004
    type: exception
    text: "Business rule detection requires BOTH an associated test file AND a non-generic comment (two-factor heuristic to reduce false positives)"
    confidence: seen-in-code
    related_files:
      - src/init/candidates/heuristics.rs
    sources:
      - kind: file
        ref: src/init/candidates/heuristics.rs
        line: 118
    status: active
    last_reviewed: "2026-03-30"
---

# Candidates

## Purpose

Generates memory item candidates by scanning source files with heuristics. Candidates are proposals that a human reviews and either promotes or rejects. This is the automated discovery step of the wiki workflow.

## Key behaviors

- Three heuristic detectors: exception (filename patterns), decision (comment keywords), business_rule (test + comment two-factor)
- Exception detection scans file paths against regex patterns for legacy/compat/override naming
- Decision detection scans the `text` field of `CodeComment` structs for deliberate-choice keywords, filtering generic text
- Business rule detection requires both a test file association and a meaningful (non-generic, non-decision) comment
- Utility files (utils, helpers, constants, config, setup, index) are excluded via regex
- Test files and directories are excluded from source scanning
- Generic text filter rejects comments shorter than 15 chars or starting with common phrases (handles, manages, processes, etc.)
- Text is truncated to 120 chars with ellipsis for candidate descriptions

## Dependencies

- [note](../note/_overview.md) (candidate types mirror MemoryItemType: Exception, Decision, BusinessRule)

## Referenced by

- [promote](../promote/_overview.md) (consumes `_candidates.md` produced by this module)
