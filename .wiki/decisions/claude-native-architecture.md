---
domain: analyze
confidence: confirmed
last_updated: "2026-04-02"
related_files:
  - src/init/analyze.rs
  - src/init/mod.rs
  - Cargo.toml
---

# Codefidence is Claude-native

## Decision

Use Claude Code CLI (`claude -p`) as the LLM backend instead of direct Anthropic API calls via reqwest.

Codefidence is explicitly **the memory and context layer for Claude Code** — not a generic LLM documentation tool.

## Reason

- Users already have Claude Code installed and authenticated — no separate API key needed
- Eliminates the worst friction point: "I'm already using Claude, why is this tool asking for a key?"
- `claude -p --output-format json --json-schema` gives us structured output with validation built-in
- Simpler code: no HTTP client, no retry logic, no token management
- Stronger product positioning: excellent for Claude Code users > mediocre for everyone

## Impact

- **Rust handles**: scan, context, check-diff, validate, promote, index, graph (deterministic)
- **Claude handles**: analyze, initial documentation, enrichment, memory candidate proposals (semantic)
- `reqwest` dependency removed from default build (kept only for `--features notion`)
- `--scan-only` flag available for structural-only bootstrap without Claude
- Clear error if `claude` CLI is not found: "Claude Code is required for AI analysis"

## Related

- [Scan domain](../domains/scan/_overview.md)
- [Candidates domain](../domains/candidates/_overview.md)
