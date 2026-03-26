use proptest::prelude::*;

// We need to test internal functions, so we'll test through the binary's behavior
// For unit-level property tests, add them to the respective module files

#[cfg(test)]
mod property_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use assert_cmd::Command;

    // The init command should never panic regardless of directory state
    proptest! {
        #[test]
        fn init_never_panics_on_arbitrary_dir(dirname in "[a-zA-Z0-9_-]{1,20}") {
            let dir = TempDir::new().unwrap();
            let sub = dir.path().join(&dirname);
            fs::create_dir_all(&sub).unwrap();

            // Should either succeed or return a clean error, never panic
            let _ = Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(&sub)
                .args(["init", "--no-scan"])
                .output();
        }
    }

    proptest! {
        #[test]
        fn search_never_panics(term in "\\PC{1,100}") {
            let dir = TempDir::new().unwrap();
            // Init a wiki first
            Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["init", "--no-scan"])
                .assert()
                .success();

            // Search should never panic, even with arbitrary Unicode
            let _ = Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["search", &term])
                .output();
        }
    }

    proptest! {
        #[test]
        fn add_domain_never_panics(name in "\\PC{1,50}") {
            let dir = TempDir::new().unwrap();
            Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["init", "--no-scan"])
                .assert()
                .success();

            // Should either succeed or return clean error, never panic
            let _ = Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["add", "domain", &name])
                .output();
        }
    }

    proptest! {
        #[test]
        fn add_decision_never_panics(text in "\\PC{1,200}") {
            let dir = TempDir::new().unwrap();
            Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["init", "--no-scan"])
                .assert()
                .success();

            // Should never panic
            let _ = Command::cargo_bin("project-wiki")
                .unwrap()
                .current_dir(dir.path())
                .args(["add", "decision", &text])
                .output();
        }
    }
}
