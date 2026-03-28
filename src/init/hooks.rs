use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::ui;

const MANAGED_BY: &str = "project-wiki";

/// Install Claude Code hooks for automatic wiki integration.
///
/// Creates or updates `.claude/settings.json` with PreToolUse and PostToolUse
/// hooks that point to `project-wiki context --hook` and `project-wiki detect-drift --hook`.
pub fn install(project_root: &Path) -> Result<()> {
    let claude_dir = project_root.join(".claude");
    fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;

    let settings_path = claude_dir.join("settings.json");

    // Load existing settings or start fresh
    let mut settings: Value = if settings_path.exists() {
        let content =
            fs::read_to_string(&settings_path).context("Failed to read .claude/settings.json")?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    // Ensure hooks object exists
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }

    // Install PreToolUse hook
    upsert_hook(
        &mut settings,
        "PreToolUse",
        "Edit|Write",
        "project-wiki context --hook",
    );

    // Install PostToolUse hook
    upsert_hook(
        &mut settings,
        "PostToolUse",
        "Edit|Write",
        "project-wiki detect-drift --hook",
    );

    // Write back
    let json = serde_json::to_string_pretty(&settings).context("Failed to serialize settings")?;
    fs::write(&settings_path, json).context("Failed to write .claude/settings.json")?;

    ui::success("Claude Code hooks installed.");
    ui::info("PreToolUse → project-wiki context --hook");
    ui::info("PostToolUse → project-wiki detect-drift --hook");

    Ok(())
}

/// Remove project-wiki hooks from `.claude/settings.json`.
pub fn uninstall(project_root: &Path) -> Result<()> {
    let settings_path = project_root.join(".claude/settings.json");

    if !settings_path.exists() {
        ui::info("No .claude/settings.json found. Nothing to uninstall.");
        return Ok(());
    }

    let content =
        fs::read_to_string(&settings_path).context("Failed to read .claude/settings.json")?;
    let mut settings: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));

    let hooks = match settings.get_mut("hooks") {
        Some(h) => h,
        None => {
            ui::info("No hooks found in settings. Nothing to uninstall.");
            return Ok(());
        }
    };

    // Remove managed entries from each hook type
    for event_type in &["PreToolUse", "PostToolUse"] {
        if let Some(entries) = hooks.get_mut(event_type) {
            if let Some(arr) = entries.as_array_mut() {
                arr.retain(|entry| !is_managed_entry(entry));
            }
        }
    }

    let json = serde_json::to_string_pretty(&settings).context("Failed to serialize settings")?;
    fs::write(&settings_path, json).context("Failed to write .claude/settings.json")?;

    ui::success("Claude Code hooks removed.");
    Ok(())
}

// ─── Helpers ───

/// Insert or update a managed hook entry in the settings.
fn upsert_hook(settings: &mut Value, event_type: &str, matcher: &str, command: &str) {
    let hooks = settings.get_mut("hooks").expect("hooks key must exist");

    // Ensure the event type array exists
    if hooks.get(event_type).is_none() {
        hooks[event_type] = json!([]);
    }

    let entries = hooks[event_type]
        .as_array_mut()
        .expect("hook event type must be an array");

    // Look for an existing managed entry
    let existing_idx = entries.iter().position(is_managed_entry);

    let new_entry = json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command,
            "_managed_by": MANAGED_BY
        }]
    });

    match existing_idx {
        Some(idx) => entries[idx] = new_entry,
        None => entries.push(new_entry),
    }
}

/// Check if a hook entry is managed by project-wiki.
fn is_managed_entry(entry: &Value) -> bool {
    if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
        return hooks
            .iter()
            .any(|hook| hook.get("_managed_by").and_then(|v| v.as_str()) == Some(MANAGED_BY));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_creates_settings_file() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();

        install(dir.path()).unwrap();

        let settings_path = dir.path().join(".claude/settings.json");
        assert!(settings_path.exists());

        let content = fs::read_to_string(&settings_path).unwrap();
        let settings: Value = serde_json::from_str(&content).unwrap();

        // Check PreToolUse
        let pre = &settings["hooks"]["PreToolUse"];
        assert!(pre.is_array());
        assert_eq!(pre.as_array().unwrap().len(), 1);
        assert_eq!(pre[0]["matcher"], "Edit|Write");
        assert_eq!(pre[0]["hooks"][0]["command"], "project-wiki context --hook");
        assert_eq!(pre[0]["hooks"][0]["_managed_by"], "project-wiki");

        // Check PostToolUse
        let post = &settings["hooks"]["PostToolUse"];
        assert!(post.is_array());
        assert_eq!(
            post[0]["hooks"][0]["command"],
            "project-wiki detect-drift --hook"
        );
    }

    #[test]
    fn install_is_idempotent() {
        let dir = TempDir::new().unwrap();

        install(dir.path()).unwrap();
        install(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        let settings: Value = serde_json::from_str(&content).unwrap();

        // Should have exactly 1 entry per event type, not 2
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn install_preserves_existing_settings() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();

        // Write existing settings with custom hooks
        let existing = json!({
            "permissions": { "allow": ["Bash(npm:*)"] },
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{ "type": "command", "command": "custom-safety-check" }]
                }]
            }
        });
        fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install(dir.path()).unwrap();

        let content = fs::read_to_string(claude_dir.join("settings.json")).unwrap();
        let settings: Value = serde_json::from_str(&content).unwrap();

        // Existing permission should be preserved
        assert_eq!(settings["permissions"]["allow"][0], "Bash(npm:*)");

        // Existing custom hook should be preserved
        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2); // custom + project-wiki
        assert!(
            pre.iter()
                .any(|e| e["hooks"][0]["command"] == "custom-safety-check")
        );
        assert!(pre.iter().any(|e| is_managed_entry(e)));
    }

    #[test]
    fn uninstall_removes_managed_hooks_only() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();

        // Install a custom hook + project-wiki hooks
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "custom-check" }] },
                    { "matcher": "Edit|Write", "hooks": [{ "type": "command", "command": "project-wiki context --hook", "_managed_by": "project-wiki" }] }
                ]
            }
        });
        fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        uninstall(dir.path()).unwrap();

        let content = fs::read_to_string(claude_dir.join("settings.json")).unwrap();
        let settings: Value = serde_json::from_str(&content).unwrap();

        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0]["hooks"][0]["command"], "custom-check");
    }

    #[test]
    fn uninstall_noop_when_no_settings() {
        let dir = TempDir::new().unwrap();
        // No .claude/settings.json exists
        assert!(uninstall(dir.path()).is_ok());
    }
}
