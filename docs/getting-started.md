# Getting started with codefidence

This guide walks you through setting up `codefidence` in an existing project. It takes about 20 minutes total, broken into independent phases you can do at your own pace.

If you just want the one-liner, `codefidence init --full` does everything at once. This guide exists because "everything at once" means scan + hooks + CLAUDE.md patching + slash commands, and that's a lot to absorb before you know what the tool actually does.

## Prerequisites

- Rust 1.85+ installed
- `codefidence` built and on your PATH:
  ```bash
  git clone https://github.com/agencedebord/codefidence.git
  cd codefidence && cargo install --path .
  ```

## Phase 1: Bootstrap (5 minutes)

```bash
cd your-project
codefidence init --scan
```

What this does:
- Creates the `.wiki/` directory structure (`domains/`, `decisions/`, `_index.md`, `_graph.md`)
- Scans your codebase to discover domains based on project structure
- Generates domain notes in `.wiki/domains/` with `inferred` confidence

After this step:
```bash
# See what was found
codefidence status

# Browse the generated notes
ls .wiki/domains/
codefidence consult --all
```

The scan works for TypeScript/JavaScript, Python, Rust, Go, Ruby, Java, and PHP projects. TypeScript is the most tested target. Even if detection is imperfect, you now have a skeleton to work with.

## Phase 2: Generate and review candidates (10 minutes)

```bash
codefidence generate-candidates
```

What this does:
- Detects patterns in your code: legacy files, decision comments (`// DECISION:`, `// HACK:`, `// FIXME:`), tested business rules
- Writes candidates to `.wiki/_candidates.md`

Review the candidates:
- Open `.wiki/_candidates.md`
- Each candidate has a type (`exception`, `decision`, or `business_rule`), text, and provenance (where it was found)
- For each one, decide: promote it, reformulate it, or reject it

Not all candidates are useful. The scanner casts a wide net on purpose. Expect noise -- that's normal.

## Phase 3: Curate your first memory items (5 minutes)

For each candidate worth keeping:
```bash
codefidence promote <candidate-id>
# Example: codefidence promote billing-001
```

Optional -- override the text or confidence level:
```bash
codefidence promote billing-001 --text "Client X uses legacy pricing engine" --confidence confirmed
```

Reject the rest:
```bash
codefidence reject billing-003
```

Three types of memory items, ordered by how dangerous they are to ignore:

| Type | What it means |
|------|---------------|
| `exception` | A deviation from the norm. An AI will try to "fix" it. Most dangerous to lose. |
| `decision` | An explicit architectural or business choice. Breaking it undoes a deliberate tradeoff. |
| `business_rule` | A known rule the code implements. Context for understanding behavior. |

You don't need to promote everything now. Start with 3-5 items you're confident about.

## Phase 4: Verify the loop works (2 minutes)

Run the three core commands to make sure your wiki is useful:

```bash
# Context: what does the wiki know about a file?
codefidence context --file src/billing/invoice.ts

# Check-diff: what memory items are affected by recent changes?
codefidence check-diff

# Validate: is the wiki healthy?
codefidence validate
```

`context` should surface relevant memory items for the file. `check-diff` (with no arguments) checks unstaged git changes. `validate` reports broken links, dead references, and staleness.

If all three produce useful output, the core loop works. If `context` returns nothing for a file you know has quirks, you need more memory items.

## Phase 5: Install hooks (optional, when ready)

```bash
codefidence init --hooks
```

This installs two Claude Code hooks:
- **PreToolUse** (`context --hook`): runs before file edits, injects relevant memory items into Claude's context
- **PostToolUse** (`detect-drift --hook`): runs after file writes, flags when a tracked file's wiki note might be stale

Only install hooks when you're comfortable with the wiki content. The hooks inject wiki memory into Claude Code's context window -- if the wiki is mostly noise or `inferred` guesses, the hooks add noise too.

To remove them later:
```bash
codefidence uninstall-hooks
```

## What NOT to do first

- **Don't install hooks before you have useful memory items.** Hooks surface whatever is in the wiki. If it's all `inferred` noise, you're polluting Claude's context for no benefit.
- **Don't try to document every domain in one session.** Start with the domains where mistakes are most costly.
- **Don't aim for `validate --strict` passing on day one.** Validation is a target, not a gate.
- **Don't promote candidates you're unsure about.** Use `reject` freely. You can always re-run `generate-candidates` later.

## Recommended pace

| Week | Goal |
|------|------|
| 1 | Bootstrap + promote 3-5 memory items across 2-3 domains |
| 2 | Install hooks, use `context` and `check-diff` daily |
| 3 | Run `validate` in CI, promote new items as you discover them |
| 4+ | Periodic review: `confirm` stale items, `deprecate` obsolete ones |

## Confidence levels

Every note and memory item carries a confidence level. This matters because it determines how much you (and Claude) should trust it:

| Level | Trust it? |
|-------|-----------|
| `confirmed` | Yes -- validated by a human |
| `verified` | Yes -- cross-checked in code and docs |
| `seen-in-code` | Mostly -- verify if the decision is critical |
| `inferred` | Maybe -- check before relying on it |
| `needs-validation` | No -- verify first |

After the initial scan, most things will be `inferred`. That's fine. Promote confidence as you verify items:

```bash
codefidence confirm billing-001
```

**Rule**: if the wiki contradicts the code, the code wins. Update the wiki.

## Next steps

- Read the full [command reference](../README.md#all-commands) in the README
- Check `.wiki/config.toml` for configuration options (staleness threshold, auto-indexing)
