Update the project wiki after completing a task.

1. Review what was changed (git diff or context from the conversation)
2. Identify which domains were affected
3. For each affected domain:
   a. Update or create `.wiki/domains/<domain>/_overview.md`
   b. Update "Key behaviors" and "Business rules" sections
   c. Update "Dependencies" and "Referenced by" cross-references
   d. Set appropriate confidence tags: `[confirmed]` for human-provided info, `[seen-in-code]` for code-derived info
   e. Update `last_updated` in front matter
4. If a non-obvious business decision was made, create a new note in `.wiki/decisions/` using the decision template
5. Regenerate `.wiki/_graph.md` based on all domain dependency sections
6. Update `.wiki/_index.md` with any new domains, decisions, or changes
7. Stage wiki changes separately and commit with prefix "wiki:"

Do NOT document:
- Standard framework/library behavior
- Implementation details obvious from the code
- Trivial bugfixes with no behavioral impact

Arguments context: $ARGUMENTS
