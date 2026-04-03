use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result};

use crate::ui;

const START_MARKER: &str = "# [codefidence:start]";
const END_MARKER: &str = "# [codefidence:end]";
const SHEBANG: &str = "#!/bin/sh";

/// Hook definitions: (hook_name, codefidence_command)
const HOOKS: &[(&str, &str)] = &[
    (
        "post-merge",
        "codefidence git-hook --event post-merge 2>/dev/null || true",
    ),
    (
        "post-rewrite",
        "codefidence git-hook --event post-rewrite 2>/dev/null || true",
    ),
    (
        "post-checkout",
        "codefidence git-hook --event post-checkout --old-ref \"$1\" --new-ref \"$2\" --branch-flag \"$3\" 2>/dev/null || true",
    ),
    (
        "post-commit",
        "codefidence git-hook --event post-commit 2>/dev/null || true",
    ),
];

/// Install git hooks for wiki drift detection.
///
/// For each hook type, creates or updates the hook script in `.git/hooks/`.
/// Uses `# [codefidence:start]` / `# [codefidence:end]` markers for idempotent
/// insertion and clean uninstallation.
pub fn install(project_root: &Path) -> Result<()> {
    let hooks_dir = project_root.join(".git/hooks");

    if !hooks_dir.exists() {
        ui::warn("No .git/hooks/ directory found. Is this a git repository?");
        return Ok(());
    }

    for (hook_name, command) in HOOKS {
        install_one_hook(&hooks_dir, hook_name, command)
            .with_context(|| format!("Failed to install {} hook", hook_name))?;
    }

    ui::success("Git hooks installed.");
    for (hook_name, _) in HOOKS {
        ui::info(&format!("  {}", hook_name));
    }

    Ok(())
}

/// Remove codefidence blocks from all git hooks.
pub fn uninstall(project_root: &Path) -> Result<()> {
    let hooks_dir = project_root.join(".git/hooks");

    if !hooks_dir.exists() {
        ui::info("No .git/hooks/ directory found. Nothing to uninstall.");
        return Ok(());
    }

    let mut removed = 0;

    for (hook_name, _) in HOOKS {
        if uninstall_one_hook(&hooks_dir, hook_name)? {
            removed += 1;
        }
    }

    if removed > 0 {
        ui::success(&format!(
            "Removed codefidence from {} git hook(s).",
            removed
        ));
    } else {
        ui::info("No codefidence git hooks found.");
    }

    Ok(())
}

// ─── Helpers ───

/// Install a single git hook, preserving any existing content.
fn install_one_hook(hooks_dir: &Path, hook_name: &str, command: &str) -> Result<()> {
    let hook_path = hooks_dir.join(hook_name);
    let codefidence_block = format!("{}\n{}\n{}\n", START_MARKER, command, END_MARKER);

    let content = if hook_path.exists() {
        let existing = fs::read_to_string(&hook_path)
            .with_context(|| format!("Failed to read {}", hook_path.display()))?;

        if existing.contains(START_MARKER) {
            // Replace existing codefidence block (idempotent update)
            replace_codefidence_block(&existing, &codefidence_block)
        } else {
            // Append codefidence block to existing hook
            let mut content = existing;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&codefidence_block);
            content
        }
    } else {
        // Create new hook file
        format!("{}\n\n{}", SHEBANG, codefidence_block)
    };

    fs::write(&hook_path, &content)
        .with_context(|| format!("Failed to write {}", hook_path.display()))?;

    // Make executable (Unix only)
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms)?;
    }

    Ok(())
}

/// Remove the codefidence block from a single hook file.
/// Returns true if a block was removed.
fn uninstall_one_hook(hooks_dir: &Path, hook_name: &str) -> Result<bool> {
    let hook_path = hooks_dir.join(hook_name);

    if !hook_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&hook_path)
        .with_context(|| format!("Failed to read {}", hook_path.display()))?;

    if !content.contains(START_MARKER) {
        return Ok(false);
    }

    let cleaned = remove_codefidence_block(&content);

    // If only shebang (or nothing useful) remains, remove the file
    let meaningful = cleaned
        .lines()
        .any(|line| !line.trim().is_empty() && line.trim() != SHEBANG);

    if meaningful {
        fs::write(&hook_path, cleaned)
            .with_context(|| format!("Failed to write {}", hook_path.display()))?;
    } else {
        fs::remove_file(&hook_path)
            .with_context(|| format!("Failed to remove {}", hook_path.display()))?;
    }

    Ok(true)
}

/// Replace the codefidence block in existing content.
fn replace_codefidence_block(content: &str, new_block: &str) -> String {
    let mut result = String::new();
    let mut in_block = false;
    let mut replaced = false;

    for line in content.lines() {
        if line.trim() == START_MARKER {
            in_block = true;
            if !replaced {
                result.push_str(new_block);
                replaced = true;
            }
            continue;
        }
        if line.trim() == END_MARKER {
            in_block = false;
            continue;
        }
        if !in_block {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Remove the codefidence block from content.
fn remove_codefidence_block(content: &str) -> String {
    let mut result = String::new();
    let mut in_block = false;

    for line in content.lines() {
        if line.trim() == START_MARKER {
            in_block = true;
            continue;
        }
        if line.trim() == END_MARKER {
            in_block = false;
            continue;
        }
        if !in_block {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_git_hooks_dir(dir: &TempDir) -> std::path::PathBuf {
        let hooks_dir = dir.path().join(".git/hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        hooks_dir
    }

    #[test]
    fn install_creates_hook_files() {
        let dir = TempDir::new().unwrap();
        create_git_hooks_dir(&dir);

        install(dir.path()).unwrap();

        for (hook_name, _) in HOOKS {
            let hook_path = dir.path().join(format!(".git/hooks/{}", hook_name));
            assert!(hook_path.exists(), "{} should exist", hook_name);

            let content = fs::read_to_string(&hook_path).unwrap();
            assert!(content.starts_with(SHEBANG));
            assert!(content.contains(START_MARKER));
            assert!(content.contains(END_MARKER));
            assert!(content.contains("codefidence git-hook"));

            // Check executable permission (Unix only)
            #[cfg(unix)]
            {
                let perms = fs::metadata(&hook_path).unwrap().permissions();
                assert_eq!(perms.mode() & 0o755, 0o755);
            }
        }
    }

    #[test]
    fn install_is_idempotent() {
        let dir = TempDir::new().unwrap();
        create_git_hooks_dir(&dir);

        install(dir.path()).unwrap();
        let content_first = fs::read_to_string(dir.path().join(".git/hooks/post-merge")).unwrap();

        install(dir.path()).unwrap();
        let content_second = fs::read_to_string(dir.path().join(".git/hooks/post-merge")).unwrap();

        assert_eq!(content_first, content_second);

        // Verify only one codefidence block exists
        let start_count = content_second.matches(START_MARKER).count();
        assert_eq!(start_count, 1, "Should have exactly one codefidence block");
    }

    #[test]
    fn install_preserves_existing_hooks() {
        let dir = TempDir::new().unwrap();
        let hooks_dir = create_git_hooks_dir(&dir);

        // Write an existing post-merge hook
        let existing_content = "#!/bin/sh\n\n# Custom hook\necho 'Running custom check'\n";
        fs::write(hooks_dir.join("post-merge"), existing_content).unwrap();

        install(dir.path()).unwrap();

        let content = fs::read_to_string(hooks_dir.join("post-merge")).unwrap();
        assert!(
            content.contains("echo 'Running custom check'"),
            "Should preserve existing hook content"
        );
        assert!(
            content.contains(START_MARKER),
            "Should add codefidence block"
        );
    }

    #[test]
    fn uninstall_removes_codefidence_blocks() {
        let dir = TempDir::new().unwrap();
        create_git_hooks_dir(&dir);

        install(dir.path()).unwrap();
        uninstall(dir.path()).unwrap();

        // Pure codefidence hooks should be deleted entirely
        for (hook_name, _) in HOOKS {
            let hook_path = dir.path().join(format!(".git/hooks/{}", hook_name));
            assert!(
                !hook_path.exists(),
                "{} should be deleted when only codefidence content",
                hook_name
            );
        }
    }

    #[test]
    fn uninstall_preserves_other_hooks() {
        let dir = TempDir::new().unwrap();
        let hooks_dir = create_git_hooks_dir(&dir);

        // Create hook with custom content + codefidence
        let content = format!(
            "#!/bin/sh\n\necho 'custom'\n\n{}\ncodefidence git-hook --event post-merge 2>/dev/null || true\n{}\n",
            START_MARKER, END_MARKER
        );
        fs::write(hooks_dir.join("post-merge"), &content).unwrap();

        uninstall(dir.path()).unwrap();

        let remaining = fs::read_to_string(hooks_dir.join("post-merge")).unwrap();
        assert!(
            remaining.contains("echo 'custom'"),
            "Should keep custom content"
        );
        assert!(
            !remaining.contains(START_MARKER),
            "Should remove codefidence block"
        );
    }

    #[test]
    fn uninstall_noop_when_no_git() {
        let dir = TempDir::new().unwrap();
        // No .git directory
        assert!(uninstall(dir.path()).is_ok());
    }

    #[test]
    fn replace_block_works() {
        let content = "#!/bin/sh\n\necho 'before'\n\n# [codefidence:start]\nold command\n# [codefidence:end]\n\necho 'after'\n";
        let new_block = "# [codefidence:start]\nnew command\n# [codefidence:end]\n";

        let result = replace_codefidence_block(content, new_block);
        assert!(result.contains("echo 'before'"));
        assert!(result.contains("new command"));
        assert!(!result.contains("old command"));
        assert!(result.contains("echo 'after'"));
    }

    #[test]
    fn remove_block_works() {
        let content = "#!/bin/sh\n\necho 'keep'\n\n# [codefidence:start]\ncodefidence stuff\n# [codefidence:end]\n\necho 'also keep'\n";

        let result = remove_codefidence_block(content);
        assert!(result.contains("echo 'keep'"));
        assert!(result.contains("echo 'also keep'"));
        assert!(!result.contains("codefidence stuff"));
        assert!(!result.contains(START_MARKER));
    }
}
