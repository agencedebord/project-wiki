# Team workflow guide

How to use `project-wiki` as a team without it becoming another thing nobody maintains.

## Who maintains the wiki?

Everyone who touches the code. The wiki is repo-native — it lives in `.wiki/` next to your source files and follows the same PR workflow as code.

**Conventions:**

- Wiki changes go in the same PR as the code they document. Use a separate commit with a `wiki:` prefix (e.g., `wiki: document billing exception for client X`).
- No dedicated "wiki owner." Ownership follows domain ownership — whoever owns `src/billing/` owns `.wiki/domains/billing/_overview.md`.
- `validate` is the quality gate. Run it in CI to catch broken links, stale notes, and orphaned references before they merge.

## When to promote / confirm / deprecate

### Promote a candidate

After `generate-candidates` scans your codebase and surfaces potential items:

- Promote when you recognize a real exception, decision, or business rule in the candidate.
- Rule of thumb: if you'd explain it verbally to a new team member, it deserves to be a memory item.
- Use `project-wiki promote <id>` to move it into the relevant domain note.

### Confirm an item

- After you've verified the behavior is still true in the code.
- During periodic review (monthly is a good cadence).
- After touching code near a memory item — if it still holds, bump its confidence with `project-wiki confirm <id>`.

### Deprecate

- When the behavior no longer exists in the code.
- When a decision has been reversed.
- Don't delete — use `project-wiki deprecate <id>` to keep the audit trail. Future readers benefit from knowing what *was* true and why it changed.

## Handling merge conflicts in .wiki/

### Source files (domain notes)

Treat like code conflicts — both sides review. The `memory_items` YAML array commonly conflicts on adjacent additions. Merge both items and ensure IDs remain unique within the domain.

### Derived files (_index.md, _graph.md, .file-index.json)

Accept either side, then run:

```bash
project-wiki rebuild
```

These files are generated — never resolve them by hand.

### Candidates file (_candidates.md)

Accept either side. Candidates are ephemeral. Re-run `generate-candidates` if needed.

## Preventing wiki debt

1. **Run `validate` in CI** — catches broken links, stale notes, orphaned references.
2. **Use `validate --strict` for releases** — promotes warnings to errors.
3. **Keep items concrete** — write "Client X uses legacy pricing via `legacy_pricing.ts`", not "there's a special case somewhere."
4. **Deprecate instead of deleting** — maintains history and audit trail.
5. **Review migration status** — `project-wiki validate` reports notes without memory items (check #11).

## Recommended PR conventions

| Scenario | Convention |
|----------|-----------|
| Wiki-only change | Single commit, `wiki:` prefix |
| Code + wiki | Separate commits in same PR |
| Large wiki refactor | Dedicated PR, no feature code mixed in |

## Minimal CI setup

```yaml
# In your CI pipeline
- name: Validate wiki
  run: project-wiki validate
```

For stricter checks before release:

```yaml
- name: Strict wiki validation
  run: project-wiki validate --strict
```
