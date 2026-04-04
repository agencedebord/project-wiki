# Domain dependency graph

> Auto-generated from domain notes. Do not edit manually.
> Last regenerated: 2026-04-04

```mermaid
graph LR
    promote
    candidates
    promote -->|(generates the `_candidates.md` file consumed here)| candidates
    add
    note
    add -->|(domain overview template structure)| note
    validate
    context
    validate -->|(shared note model via `src/wiki/note.rs`)| context
    candidates -->|(candidate types mirror MemoryItemType: Exception, Decision, BusinessRule)| note
    check_diff["check-diff"]
    check_diff -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| context
    context -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| check_diff
    context -->|(shared note parsing via `src/wiki/note.rs`)| validate
    drift
    drift -->|(WikiNote, Confidence, MemoryItemStatus types)| note
    drift -->|(shared file_index resolution)| context
    manage
    manage -->|(WikiNote parsing and writing for confirm/deprecate)| note
    scan

    style context fill:#e74c3c,color:#fff
    style note fill:#e74c3c,color:#fff
    style check_diff fill:#e74c3c,color:#fff
    style validate fill:#e74c3c,color:#fff
    style candidates fill:#e74c3c,color:#fff
    style drift fill:#e74c3c,color:#fff
```
