# Domain dependency graph

> Auto-generated from domain notes. Do not edit manually.
> Last regenerated: 2026-03-31

```mermaid
graph LR
    drift
    note
    drift -->|(WikiNote, Confidence, MemoryItemStatus types)| note
    context
    drift -->|(shared file_index resolution)| context
    check_diff["check-diff"]
    drift -->|(complementary: check-diff analyzes multiple files, drift analyzes one)| check_diff
    add
    add -->|(domain overview template structure)| note
    manage
    manage -->|(WikiNote parsing and writing for confirm/deprecate)| note
    context -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| check_diff
    validate
    context -->|(shared note parsing via `src/wiki/note.rs`)| validate
    validate -->|(shared note model via `src/wiki/note.rs`)| check_diff
    validate -->|(shared note model via `src/wiki/note.rs`)| context
    promote
    promote -->|(WikiNote, MemoryItem, Confidence types)| note
    candidates
    promote -->|(generates the `_candidates.md` file consumed here)| candidates
    candidates -->|(candidate types mirror MemoryItemType: Exception, Decision, BusinessRule)| note
    check_diff -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| context
    check_diff -->|(shared note parsing via `src/wiki/note.rs`)| validate
    scan

    style check_diff fill:#e74c3c,color:#fff
    style promote fill:#e74c3c,color:#fff
    style validate fill:#e74c3c,color:#fff
    style context fill:#e74c3c,color:#fff
    style candidates fill:#e74c3c,color:#fff
    style drift fill:#e74c3c,color:#fff
    style note fill:#e74c3c,color:#fff
```
