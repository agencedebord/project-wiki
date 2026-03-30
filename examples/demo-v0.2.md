# project-wiki v0.2 Demo Scenarios

> Captured on 2026-03-30 against the project-wiki codebase itself.
> The wiki has 3 domain notes (check-diff, context, validate) with 13 memory items total.

---

## Demo 1: "context helped me avoid an error"

**Scenario:** A developer is about to modify `src/wiki/check_diff/resolve.rs`.
Before touching the file, they (or a Claude Code hook) ask the wiki what they
need to know.

```
$ cargo run -- context --file src/wiki/check_diff/resolve.rs
```

**Output:**

```
[project-wiki] Domain: check-diff (confidence: seen-in-code, updated: 2026-03-30)
Memory:
  [exception] Deprecated memory items are filtered out before prioritization and never appear in output [seen-in-code]
  [decision] Maximum 3 domains shown in output, sorted by file count then memory_items count [seen-in-code]
  [decision] Item prioritization order: exception > decision > business_rule, then confidence rank, then related files match [seen-in-code]
  (+1 more items)
Dependencies: [context](../context/_overview.md) (shared prioritization logic via `src/wiki/prioritize.rs`), [validate](../validate/_overview.md) (shared note parsing via `src/wiki/note.rs`)
Related files: src/wiki/check_diff/mod.rs, src/wiki/check_diff/resolve.rs, src/wiki/check_diff/sensitivity.rs, src/wiki/check_diff/prioritize.rs
```

**What this shows:** Before making any change, the developer is reminded that:
- Deprecated items are silently filtered (an exception they might forget)
- Output is capped at 3 domains with a specific sort order (a decision)
- Prioritization follows a precise cascade (exception > decision > business_rule)

Without this, they might accidentally break the 3-domain limit or change the
sort order without realizing it was an intentional design decision.

---

## Demo 2: "check-diff reminds me of a decision"

**Scenario:** The developer has uncommitted changes across several files,
including modifications to `src/wiki/check_diff/sensitivity.rs`. They run
check-diff to see if any documented decisions are affected.

```
$ cargo run -- check-diff
```

**Output:**

```
[project-wiki] Diff check

14 file(s) analyzed
2 domain(s) affected
Sensitivity: high

Affected domains
  check-diff (primary) -- 1 file(s), 3 item(s)
  validate (secondary) -- 1 file(s), 3 item(s)

Priority memory
  check-diff:
    [exception] Deprecated memory items are filtered out before prioritization
                and never appear in output [seen-in-code]
    [decision]  Maximum 3 domains shown in output, sorted by file count then
                memory_items count [seen-in-code]
    [decision]  Item prioritization order: exception > decision > business_rule,
                then confidence rank, then related files match [seen-in-code]
  validate:
    [exception] Memory items validation is check #10, added last to the
                validation pipeline; checks duplicate IDs, source integrity,
                confidence consistency, and future dates [seen-in-code]
    [decision]  Strict mode promotes all warnings to errors, causing non-zero
                exit on any issue [seen-in-code]
    [business_rule] 10 validation checks run in order: broken links,
                undocumented domains, dead refs, deprecated refs, confidence
                ratio, staleness, orphan notes, domain name coherence,
                cross-domain deps, memory items [seen-in-code]

Suggested actions
  -> Verify exception 'Deprecated memory items are filtered out...' still holds
  -> Verify decision 'Maximum 3 domains shown in output...' still holds
  -> Verify decision 'Item prioritization order...' still holds

Unresolved files
  README.md
  src/init/hooks.rs
  src/init/scan/generate.rs
  src/wiki/add.rs
  src/wiki/check_diff/tests.rs
  src/wiki/config.rs
  src/wiki/context/tests.rs
  src/wiki/drift.rs
  src/wiki/note.rs
  src/wiki/search.rs
  src/wiki/validate/tests.rs
  tests/cli_tests.rs
```

**What this shows:** The diff touched check-diff and validate domains. The tool
surfaces the most important memory items (exceptions first, then decisions, then
business rules) and suggests concrete verification actions. "Unresolved files"
lists changed files that are not yet mapped to any domain -- candidates for
future documentation.

---

## Demo 3: "candidate -> promote -> exploitable memory"

**Scenario:** The developer runs `generate-candidates` to discover undocumented
patterns in the codebase, then promotes the best ones to confirmed memory items.

### Step 1: Generate candidates

```
$ cargo run -- generate-candidates
```

**Output:**

```
Scanning codebase

  Pass 1 -- discovering project structure...
  ██████████░░░░░░░░░░░░░░░░░░░░  33%  86 files found, 5 languages detected
  No domain candidates found. The wiki will start empty.
  No memory candidates detected from scan.
```

Since the wiki already documents the 3 active domains, the scanner finds no new
candidates to suggest. In a fresh or partially-documented project, this would
list domain and memory candidates with IDs.

### Step 2: Promote a candidate (hypothetical)

```
$ cargo run -- promote billing-001
```

If a candidate existed, the command would:
1. Move the candidate from `.wiki/candidates/` to the target domain's `memory_items`
2. Set confidence to `confirmed` (overridable with `--confidence`)
3. Optionally reformulate the text with `--text "cleaner wording"`

```
$ cargo run -- promote --help

Promote a memory candidate to a confirmed memory item

Usage: project-wiki promote [OPTIONS] <CANDIDATE_ID>

Arguments:
  <CANDIDATE_ID>  Candidate ID (e.g. billing-001)

Options:
      --confidence <CONFIDENCE>  Confidence level (default: confirmed)
      --text <TEXT>              Override candidate text with a reformulation
  -v, --verbose...               Increase verbosity (-v, -vv, -vvv)
  -h, --help                     Print help
```

---

## Demo 4: "validate shows migration status"

**Scenario:** The developer wants a full health check of the wiki, including
whether all notes have been migrated to the new `memory_items` format.

```
$ cargo run -- validate
```

**Output:**

```
project-wiki v0.1.0

Validating wiki

  Broken links
    No broken links found.

  Undocumented domains
    All code domains are documented.

  Dead references
    All related_files references are valid.

  Deprecated references
    No active notes link to deprecated notes.

  Confidence ratio
    0/3 notes (0%) are inferred or needs-validation -- within threshold

  Staleness
    No stale notes (all updated within 30 days).

  Orphan notes
    All domain notes are referenced in _index.md.

  Domain name coherence
    All domain folder names match note domain fields.

  Cross-domain dependencies
    All referenced dependencies exist in wiki.

  Memory items
    13 memory item(s) across 3 note(s) -- all valid.

  Migration status
    All 3 note(s) have memory_items.

  Summary
    11 passed  0 warnings  0 errors

Validation passed.
```

**What this shows:** The validate command runs 11 checks in sequence:

| # | Check | Purpose |
|---|-------|---------|
| 1 | Broken links | Internal wiki links resolve |
| 2 | Undocumented domains | No code domain lacks a wiki note |
| 3 | Dead references | `related_files` point to existing files |
| 4 | Deprecated references | Active notes don't link to deprecated ones |
| 5 | Confidence ratio | Not too many `[inferred]` notes |
| 6 | Staleness | No notes older than 30 days without update |
| 7 | Orphan notes | Every note appears in `_index.md` |
| 8 | Domain name coherence | Folder names match note metadata |
| 9 | Cross-domain deps | Dependency links resolve |
| 10 | Memory items | Item format, IDs, confidence, dates are valid |
| 11 | Migration status | All notes have `memory_items` (new in v0.2) |

The **Migration status** check (#11) is new -- it flags notes that still lack
`memory_items`, helping teams track their migration to the structured memory
format.
