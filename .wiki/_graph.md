# Domain dependency graph

> Auto-generated from domain notes. Do not edit manually.
> Last regenerated: 2026-04-07

```mermaid
graph LR
    add
    note
    add -->|(domain overview template structure)| note
    candidates
    candidates -->|(candidate types mirror MemoryItemType: Exception, Decision, BusinessRule)| note
    check_diff["check-diff"]
    validate
    check_diff -->|(shared note parsing via `src/wiki/note.rs`)| validate
    context
    context -->|(shared note parsing via `src/wiki/note.rs`)| validate
    drift
    drift -->|(WikiNote, Confidence, MemoryItemStatus types)| note
    drift -->|(shared file_index resolution)| context
    manage
    manage -->|(WikiNote parsing and writing for confirm/deprecate)| note
    promote
    promote -->|(generates the `_candidates.md` file consumed here)| candidates
    validate -->|(shared note model via `src/wiki/note.rs`)| check_diff
    validate -->|(shared note model via `src/wiki/note.rs`)| context
    scan

    style candidates fill:#e74c3c,color:#fff
    style check_diff fill:#e74c3c,color:#fff
    style context fill:#e74c3c,color:#fff
    style drift fill:#e74c3c,color:#fff
    style note fill:#e74c3c,color:#fff
    style validate fill:#e74c3c,color:#fff
```
