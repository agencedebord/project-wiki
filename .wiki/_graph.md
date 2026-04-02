# Domain dependency graph

> Auto-generated from domain notes. Do not edit manually.
> Last regenerated: 2026-04-02

```mermaid
graph LR
    promote
    note
    promote -->|(WikiNote, MemoryItem, Confidence types)| note
    candidates
    promote -->|(generates the `_candidates.md` file consumed here)| candidates
    drift
    drift -->|(WikiNote, Confidence, MemoryItemStatus types)| note
    context
    drift -->|(shared file_index resolution)| context
    check_diff["check-diff"]
    drift -->|(complementary: check-diff analyzes multiple files, drift analyzes one)| check_diff
    check_diff -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| context
    validate
    check_diff -->|(shared note parsing via `src/wiki/note.rs`)| validate
    validate -->|(shared note model via `src/wiki/note.rs`)| check_diff
    validate -->|(shared note model via `src/wiki/note.rs`)| context
    add
    add -->|(domain overview template structure)| note
    context -->|(shared prioritization logic via `src/wiki/prioritize.rs`)| check_diff
    context -->|(shared note parsing via `src/wiki/note.rs`)| validate
    manage
    manage -->|(WikiNote parsing and writing for confirm/deprecate)| note
    candidates -->|(candidate types mirror MemoryItemType: Exception, Decision, BusinessRule)| note
    scan

    style check_diff fill:#e74c3c,color:#fff
    style note fill:#e74c3c,color:#fff
    style drift fill:#e74c3c,color:#fff
    style promote fill:#e74c3c,color:#fff
    style validate fill:#e74c3c,color:#fff
    style candidates fill:#e74c3c,color:#fff
    style context fill:#e74c3c,color:#fff
```
