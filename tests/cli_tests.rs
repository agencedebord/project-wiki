use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to get a Command pointing at our binary, with cwd set to the given dir.
fn cmd_in(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("project-wiki").unwrap();
    cmd.current_dir(dir.path());
    cmd
}

// ─── init ───

#[test]
fn init_creates_expected_directory_structure() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    // .wiki/ root files
    assert!(dir.path().join(".wiki").exists());
    assert!(dir.path().join(".wiki/_index.md").exists());
    assert!(dir.path().join(".wiki/_graph.md").exists());
    assert!(dir.path().join(".wiki/_needs-review.md").exists());

    // Templates
    assert!(dir.path().join(".wiki/_templates").is_dir());
    assert!(dir.path().join(".wiki/_templates/domain-overview.md").exists());
    assert!(dir.path().join(".wiki/_templates/decision.md").exists());

    // Domain and decision directories
    assert!(dir.path().join(".wiki/domains").is_dir());
    assert!(dir.path().join(".wiki/decisions").is_dir());

    // Claude commands
    assert!(dir.path().join(".claude/commands").is_dir());
    assert!(dir.path().join(".claude/commands/wiki-consult.md").exists());
    assert!(dir.path().join(".claude/commands/wiki-update.md").exists());
    assert!(dir.path().join(".claude/commands/wiki-add-context.md").exists());
    assert!(dir.path().join(".claude/commands/wiki-add-decision.md").exists());
}

#[test]
fn init_creates_claude_md_with_project_wiki_section() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    let claude_md = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("Project Wiki"));
}

#[test]
fn init_creates_gitignore_with_wiki_entries() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".wiki/.env"));
    assert!(gitignore.contains(".wiki/.vectors/"));
}

#[test]
fn init_with_source_files_runs_scan_and_creates_domains() {
    let dir = TempDir::new().unwrap();

    // Create a project structure that will be detected as a domain
    let billing_dir = dir.path().join("src/services/billing");
    fs::create_dir_all(&billing_dir).unwrap();
    fs::write(billing_dir.join("invoice.ts"), "export class Invoice {}").unwrap();

    cmd_in(&dir).arg("init").assert().success();

    // The scan should have detected the "billing" domain
    assert!(dir.path().join(".wiki/domains/billing/_overview.md").exists());
}

#[test]
fn init_no_scan_skips_domain_creation() {
    let dir = TempDir::new().unwrap();

    // Create source files
    let billing_dir = dir.path().join("src/services/billing");
    fs::create_dir_all(&billing_dir).unwrap();
    fs::write(billing_dir.join("invoice.ts"), "export class Invoice {}").unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    // domains/ should exist but contain only .gitkeep
    let domains_dir = dir.path().join(".wiki/domains");
    assert!(domains_dir.is_dir());

    let entries: Vec<_> = fs::read_dir(&domains_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != ".gitkeep")
        .collect();
    assert!(entries.is_empty(), "Expected no domain dirs, found {:?}", entries);
}

#[test]
fn init_fails_if_wiki_already_exists() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".wiki")).unwrap();

    cmd_in(&dir)
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains(".wiki/ already exists"));
}

// ─── status ───

#[test]
fn status_works_on_initialized_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .arg("status")
        .assert()
        .success()
        .stderr(predicate::str::contains("Domains"))
        .stderr(predicate::str::contains("Notes"))
        .stderr(predicate::str::contains("Decisions"));
}

#[test]
fn status_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── validate ───

#[test]
fn validate_works_on_initialized_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .arg("validate")
        .assert()
        .success()
        .stderr(predicate::str::contains("passed"));
}

#[test]
fn validate_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── rebuild ───

#[test]
fn rebuild_regenerates_graph_and_index() {
    let dir = TempDir::new().unwrap();

    // Init with scan (create a domain so there is something to rebuild)
    let billing_dir = dir.path().join("src/services/billing");
    fs::create_dir_all(&billing_dir).unwrap();
    fs::write(billing_dir.join("invoice.ts"), "export class Invoice {}").unwrap();

    cmd_in(&dir).arg("init").assert().success();

    // Verify _graph.md was created with content
    let graph_before = fs::read_to_string(dir.path().join(".wiki/_graph.md")).unwrap();
    assert!(!graph_before.is_empty());

    // Empty the graph file
    fs::write(dir.path().join(".wiki/_graph.md"), "").unwrap();

    // Run rebuild
    cmd_in(&dir).arg("rebuild").assert().success();

    // Verify _graph.md was regenerated
    let graph_after = fs::read_to_string(dir.path().join(".wiki/_graph.md")).unwrap();
    assert!(!graph_after.is_empty());
    assert!(graph_after.contains("graph"));
}

// ─── graph ───

#[test]
fn graph_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .arg("graph")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── index ───

#[test]
fn index_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .arg("index")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

#[test]
fn index_regenerates_on_initialized_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir).arg("index").assert().success();

    let index = fs::read_to_string(dir.path().join(".wiki/_index.md")).unwrap();
    assert!(index.contains("Project Wiki"));
}

// ─── search ───

#[test]
fn search_finds_content_in_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    // Create a domain with searchable content
    let domain_dir = dir.path().join(".wiki/domains/billing");
    fs::create_dir_all(&domain_dir).unwrap();
    fs::write(
        domain_dir.join("_overview.md"),
        "---\ntitle: Billing\nconfidence: confirmed\nlast_updated: \"2026-03-26\"\nrelated_files: []\n---\n\n# Billing\n\nHandles invoice processing and payments.\n",
    ).unwrap();

    cmd_in(&dir)
        .args(["search", "invoice"])
        .assert()
        .success()
        .stderr(predicate::str::contains("invoice"));
}

#[test]
fn search_no_results() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .args(["search", "xyznonexistent"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No matches found"));
}

#[test]
fn search_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .args(["search", "test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── add domain ───

#[test]
fn add_domain_creates_the_domain() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .args(["add", "domain", "billing"])
        .assert()
        .success()
        .stderr(predicate::str::contains("billing"));

    assert!(dir.path().join(".wiki/domains/billing/_overview.md").exists());

    let content = fs::read_to_string(dir.path().join(".wiki/domains/billing/_overview.md")).unwrap();
    assert!(content.contains("domain: billing"));
}

#[test]
fn add_domain_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .args(["add", "domain", "billing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── add decision ───

#[test]
fn add_decision_creates_decision_file() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .args(["add", "decision", "Use Stripe for payments"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Decision created"));

    // Check that a decision file was created in .wiki/decisions/
    let decisions_dir = dir.path().join(".wiki/decisions");
    let entries: Vec<_> = fs::read_dir(&decisions_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "md")
        })
        .collect();

    assert_eq!(entries.len(), 1, "Expected exactly one decision file");

    let filename = entries[0].file_name().to_string_lossy().to_string();
    assert!(filename.contains("use-stripe"), "Filename '{}' should contain 'use-stripe'", filename);

    let content = fs::read_to_string(entries[0].path()).unwrap();
    assert!(content.contains("Use Stripe for payments"));
}

#[test]
fn add_decision_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .args(["add", "decision", "some text"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}

// ─── add context ───

#[test]
fn add_context_with_domain_flag() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    // First create a domain
    cmd_in(&dir)
        .args(["add", "domain", "auth"])
        .assert()
        .success();

    // Add context to it
    cmd_in(&dir)
        .args(["add", "context", "--domain", "auth", "Passwords must be hashed with bcrypt"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Added context"));

    let content = fs::read_to_string(dir.path().join(".wiki/domains/auth/_overview.md")).unwrap();
    assert!(content.contains("Passwords must be hashed with bcrypt [confirmed]"));
}

// ─── consult ───

#[test]
fn consult_all_succeeds_on_initialized_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    // Create a domain so there's something to show
    let domain_dir = dir.path().join(".wiki/domains/billing");
    fs::create_dir_all(&domain_dir).unwrap();
    fs::write(
        domain_dir.join("_overview.md"),
        "---\ntitle: Billing\nconfidence: confirmed\n---\n## Description\n\nHandles billing.\n",
    )
    .unwrap();

    cmd_in(&dir)
        .args(["consult", "--all"])
        .assert()
        .success()
        .stderr(predicate::str::contains("All domains"))
        .stdout(predicate::str::contains("Confidence: confirmed"));
}

#[test]
fn consult_nonexistent_domain_fails() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .args(["consult", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn consult_without_args_shows_overview() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    cmd_in(&dir)
        .arg("consult")
        .assert()
        .success()
        .stderr(predicate::str::contains("Wiki overview"));
}

#[test]
fn consult_specific_domain_succeeds() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir).arg("init").arg("--no-scan").assert().success();

    let domain_dir = dir.path().join(".wiki/domains/auth");
    fs::create_dir_all(&domain_dir).unwrap();
    fs::write(
        domain_dir.join("_overview.md"),
        "---\ntitle: Auth overview\nconfidence: verified\nrelated_files:\n  - src/auth.ts\n---\n## Description\n\nHandles authentication.\n",
    )
    .unwrap();

    cmd_in(&dir)
        .args(["consult", "auth"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Domain: auth"))
        .stdout(predicate::str::contains("Confidence: verified"))
        .stdout(predicate::str::contains("src/auth.ts"));
}

#[test]
fn consult_fails_without_wiki() {
    let dir = TempDir::new().unwrap();

    cmd_in(&dir)
        .args(["consult", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .wiki/ found"));
}
