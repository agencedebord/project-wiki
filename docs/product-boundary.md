# Product boundary

> "Le plus grand risque du projet maintenant, ce n'est plus l'absence de coeur. C'est la dispersion."

## Core loop (protect at all costs)

The entire product serves one loop:

1. **Memory** — structured items (exceptions, decisions, business rules) linked to files
2. **Context** — surface relevant memory before editing
3. **Check** — flag affected decisions after changes
4. **Validate** — ensure wiki health and migration progress
5. **Curate** — promote, confirm, deprecate over time

Every feature must strengthen this loop. If it doesn't, it's out of scope.

## In scope

- Improving memory item quality and curation flow
- Making context/check-diff output more useful
- Better validation checks
- Smoother onboarding and team workflow
- TypeScript/JavaScript as primary tested target
- Basic CI integration (validate, check-diff --pr-comment)
- Claude Code hook integration

## Out of scope (until the core is excellent)

- Deep multi-language AST analysis (Python, Rust, Go beyond basic scan)
- Semantic diff analysis (line-by-line patch interpretation)
- AI-powered candidate generation (LLM-based, beyond pattern heuristics)
- Slack/Discord/external notification integrations
- Interactive TUI or web UI
- Published GitHub Action (keep as documentation for now)
- Automated review comments beyond basic PR comment
- Real-time collaboration features
- Plugin/extension system

## Decision criteria for new features

Before adding anything, ask:

1. Does it make the core loop better for a single developer on a TypeScript project?
2. Can it be explained in one sentence?
3. Does it have a test that proves it works?
4. Does it avoid adding a new dependency?

If any answer is "no", defer it.

## What "better" means for each part of the loop

| Part | Better means | Not better |
|------|-------------|------------|
| Memory | More precise items, less noise | More item types, more metadata fields |
| Context | More relevant items surfaced | Longer output, more information |
| Check | Fewer false positives, clearer actions | More sophisticated analysis |
| Validate | More useful checks, clearer messages | More checks for edge cases |
| Curate | Fewer steps to promote/reject | AI auto-curation |
