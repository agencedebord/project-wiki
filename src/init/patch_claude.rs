use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

const WIKI_SECTION_MARKER: &str = "## Project Wiki (auto-managed)";

const WIKI_SECTION: &str = r#"
## Project Wiki (auto-managed)

### Before each non-trivial task
1. Read `.wiki/_index.md` for the overview
2. Read the domain notes related to the task
3. Account for documented business decisions
4. Check the dependency graph if multiple domains are involved

### After each task that modifies behavior
1. Update or create notes in `.wiki/domains/`
2. Document any non-obvious business decision in `.wiki/decisions/`
3. Update "Dependencies" and "Referenced by" sections
4. Regenerate `.wiki/_graph.md`
5. Update `.wiki/_index.md`
6. Separate commit with "wiki:" prefix

### What to document
- Counter-intuitive or client-specific business decisions
- Behaviors intentionally different from standard conventions
- Business rules not obvious from reading the code
- Domain architecture and responsibilities

### What NOT to document
- Standard framework/library behavior
- Implementation details readable in the code
- Trivial bugfixes with no behavioral impact

### Confidence rules
- `[confirmed]` or `[verified]`: trust as source of truth
- `[inferred]` or `[needs-validation]`: ALWAYS verify in code before relying on it
- If wiki contradicts code: code wins, update the wiki
- If `[inferred]` info drives a structural decision: ask the user for confirmation
"#;

pub fn run() -> Result<()> {
    run_at(Path::new("CLAUDE.md"))
}

pub fn run_at(claude_md_path: &Path) -> Result<()> {
    let mut content = if claude_md_path.exists() {
        fs::read_to_string(claude_md_path).context("Failed to read CLAUDE.md")?
    } else {
        String::new()
    };

    if content.contains(WIKI_SECTION_MARKER) {
        return Ok(());
    }

    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str(WIKI_SECTION);

    fs::write(claude_md_path, content).context("Failed to write CLAUDE.md")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_claude_md_if_not_exists() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");

        run_at(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains(WIKI_SECTION_MARKER));
        assert!(content.contains("Project Wiki"));
    }

    #[test]
    fn appends_to_existing_claude_md() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");

        fs::write(&path, "# My Project\n\nExisting content.\n").unwrap();

        run_at(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("# My Project"));
        assert!(content.contains("Existing content."));
        assert!(content.contains(WIKI_SECTION_MARKER));
    }

    #[test]
    fn does_not_duplicate_section() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");

        // Run twice
        run_at(&path).unwrap();
        run_at(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let count = content.matches(WIKI_SECTION_MARKER).count();
        assert_eq!(count, 1, "Section marker should appear exactly once");
    }

    #[test]
    fn appends_newline_to_content_not_ending_with_newline() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");

        fs::write(&path, "No trailing newline").unwrap();

        run_at(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // The original content should be separated from the wiki section
        assert!(content.contains("No trailing newline\n"));
        assert!(content.contains(WIKI_SECTION_MARKER));
    }
}
