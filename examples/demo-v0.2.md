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

> **Note:** The outputs below are representative of what each command produces on
> a partially-documented codebase. They were not captured live because
> project-wiki's own wiki is already fully documented, leaving no candidates to
> discover. The format, structure, and behavior shown are accurate.

**Scenario:** A team has just installed project-wiki on an existing SaaS
codebase. The wiki has a single `auth` domain documented, but the codebase also
contains billing, notifications, and onboarding logic. The developer runs
`generate-candidates` to discover what else should be documented, then promotes
the best findings to confirmed memory items.

### Step 1: Generate candidates

```
$ project-wiki generate-candidates
```

```
Scanning codebase

  Pass 1 -- discovering project structure...
  ████████████████████████████████  100%  142 files found, 3 languages detected

  Domain candidates: 5 (billing, payments, notifications, onboarding, analytics)
  Memory candidates: 8

Candidates written to .wiki/_candidates.md
```

The scanner analyzed the full codebase, detected 5 undocumented domain clusters,
and extracted 8 memory candidates (business rules, decisions, and exceptions it
found in code comments, config files, and structural patterns).

### Step 2: Review candidates

The developer opens `.wiki/_candidates.md` to review what was found:

```markdown
# Candidates

## Domain candidates

- **billing** (3 memory candidates)
  Files: src/billing/invoice.rs, src/billing/subscription.rs, src/billing/webhook.rs

- **notifications** (2 memory candidates)
  Files: src/notifications/dispatch.rs, src/notifications/templates.rs

- **onboarding** (1 memory candidate)
  Files: src/onboarding/checklist.rs, src/onboarding/trial.rs

[... 2 more domains ...]

## Memory candidates

- `billing-001` [decision] Invoices are generated on the 1st of each month at
  00:00 UTC regardless of subscription start date
  Related: src/billing/invoice.rs

- `billing-002` [exception] Free-tier users still get an invoice record with
  amount=0 to maintain audit trail continuity
  Related: src/billing/invoice.rs, src/billing/subscription.rs

- `billing-003` [business_rule] Webhook retries use exponential backoff: 1min,
  5min, 30min, 2h, then dead-letter queue
  Related: src/billing/webhook.rs

- `notifications-001` [decision] Email dispatch is always async via job queue,
  never inline, even for critical alerts
  Related: src/notifications/dispatch.rs

[... 4 more candidates ...]
```

Each candidate has an ID, a type (decision / exception / business_rule), a
description inferred from the code, and the files it was found in.

### Step 3: Promote a candidate

The developer decides `billing-002` is a critical exception that future
contributors must know about. They promote it:

```
$ project-wiki promote billing-002
```

```
✓ Created domain note .wiki/domains/billing/_overview.md
✓ Promoted billing-002 to .wiki/domains/billing/_overview.md (confidence: confirmed)
  [exception] Free-tier users still get an invoice record with amount=0
              to maintain audit trail continuity
```

The command created the `billing` domain note (since it did not exist yet) and
added the memory item with `confirmed` confidence. The candidate is removed from
`_candidates.md`.

To promote with a different confidence level or reworded text:

```
$ project-wiki promote notifications-001 --confidence seen-in-code --text "All email dispatch goes through the async job queue, never sent inline"
```

### Step 4: Verify the promoted item is exploitable

Now that the billing domain exists with a memory item, `context` surfaces it
when a developer touches billing files:

```
$ project-wiki context --file src/billing/invoice.rs
```

```
[project-wiki] Domain: billing (confidence: confirmed, updated: 2026-03-30)
Memory:
  [exception] Free-tier users still get an invoice record with amount=0
              to maintain audit trail continuity [confirmed]
Related files: src/billing/invoice.rs, src/billing/subscription.rs, src/billing/webhook.rs
```

**What this shows:** The full lifecycle from discovery to daily use:
1. `generate-candidates` scans the codebase and surfaces undocumented patterns
2. The developer reviews candidates and decides which ones matter
3. `promote` turns a candidate into a confirmed memory item in the wiki
4. `context` (and `check-diff`) immediately start surfacing that knowledge

This is the core feedback loop: scan -> review -> promote -> exploit. Over time,
the wiki accumulates the decisions and exceptions that are hardest to discover
by reading code alone.

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
