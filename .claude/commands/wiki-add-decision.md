Document a business decision in the project wiki.

The user described this decision: $ARGUMENTS

1. Read `.wiki/_index.md` to understand existing domains
2. Determine which domain this decision belongs to
3. Create a new decision note in `.wiki/decisions/` with today's date prefix
4. Use the decision template format:
   - Decision: what was decided (reformulate clearly)
   - Reason: why (extract from the user's input or ask)
   - Impact: what this affects in the codebase
   - Related: link to relevant domain notes
5. Mark as `[confirmed]`
6. Update the domain's `_overview.md` to reference this decision
7. Update `.wiki/_index.md` with the new decision
