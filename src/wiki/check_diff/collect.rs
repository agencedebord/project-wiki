use anyhow::{Result, bail};

/// Collect the list of modified files to check.
///
/// If `files` is non-empty, use those directly (explicit mode).
/// Otherwise, run `git diff --name-only` (or `--cached` when `staged`).
pub(super) fn collect_files(files: &[String], staged: bool) -> Result<Vec<String>> {
    if !files.is_empty() {
        let mut result = Vec::new();
        for f in files {
            let normalized = normalize_path(f);
            if should_ignore(&normalized) {
                continue;
            }
            if std::path::Path::new(&normalized).exists() {
                result.push(normalized);
            } else {
                eprintln!("warning: file not found, skipping: {normalized}");
            }
        }
        return Ok(result);
    }

    // Git diff mode
    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff").arg("--name-only");
    if staged {
        cmd.arg("--cached");
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .filter(|l| std::path::Path::new(l).exists())
        .filter(|l| !should_ignore(l))
        .collect();

    Ok(result)
}

pub(super) fn normalize_path(path: &str) -> String {
    let p = path.strip_prefix("./").unwrap_or(path);
    p.to_string()
}

pub(super) fn should_ignore(path: &str) -> bool {
    let ignored_prefixes = [
        ".wiki/",
        "node_modules/",
        "target/",
        "dist/",
        ".git/",
        "vendor/",
        "__pycache__/",
    ];
    for prefix in &ignored_prefixes {
        if path.starts_with(prefix) {
            return true;
        }
    }

    let ignored_extensions = [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".svg", ".woff", ".woff2", ".ttf", ".eot", ".mp3",
        ".mp4", ".zip", ".tar", ".gz", ".pdf", ".exe", ".dll", ".so", ".dylib",
    ];
    for ext in &ignored_extensions {
        if path.ends_with(ext) {
            return true;
        }
    }

    false
}
