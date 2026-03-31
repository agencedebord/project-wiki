mod dependencies;
mod details;
mod generate;
mod imports;
pub mod structure;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::ui;

pub use generate::{
    generate_domain_overview, generate_graph, generate_index, generate_needs_review,
};

// ─── Public types ───

#[derive(Debug, Clone)]
pub struct DomainInfo {
    pub name: String,
    pub files: Vec<String>,
    pub dependencies: Vec<String>,
    pub models: Vec<String>,
    pub routes: Vec<String>,
    pub comments: Vec<String>,
    pub test_files: Vec<String>,
}

#[derive(Debug)]
pub struct ScanResult {
    pub domains: Vec<DomainInfo>,
    pub total_files_scanned: usize,
    pub languages_detected: Vec<String>,
}

impl DomainInfo {
    /// Returns true if the domain has at least one meaningful signal
    /// beyond just having source files.
    pub fn has_signal(&self) -> bool {
        !self.models.is_empty()
            || !self.routes.is_empty()
            || !self.comments.is_empty()
            || !self.test_files.is_empty()
            || !self.dependencies.is_empty()
    }

    /// Returns a factual structural summary string.
    /// Example: "This domain contains 4 source files, 2 test files, and 3 detected models."
    pub fn structural_description(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        let source_count = self.files.len().saturating_sub(self.test_files.len());
        parts.push(format!(
            "{} source file{}",
            source_count,
            plural(source_count)
        ));

        if !self.test_files.is_empty() {
            parts.push(format!(
                "{} test file{}",
                self.test_files.len(),
                plural(self.test_files.len())
            ));
        }
        if !self.models.is_empty() {
            parts.push(format!(
                "{} detected model{}",
                self.models.len(),
                plural(self.models.len())
            ));
        }
        if !self.routes.is_empty() {
            parts.push(format!(
                "{} API route{}",
                self.routes.len(),
                plural(self.routes.len())
            ));
        }
        if !self.dependencies.is_empty() {
            let dep_names = self.dependencies.join(", ");
            parts.push(format!(
                "{} dependenc{} on {}",
                self.dependencies.len(),
                if self.dependencies.len() == 1 {
                    "y"
                } else {
                    "ies"
                },
                dep_names
            ));
        }

        format!("This domain contains {}.", join_natural(&parts))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn join_natural(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => {
            let (last, rest) = parts.split_last().unwrap();
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

// ─── Main entry point ───

pub fn run() -> Result<ScanResult> {
    let project_root = std::env::current_dir().context("Failed to get current directory")?;

    ui::action("Scanning codebase");
    eprintln!();

    // Pass 1: Structure discovery
    ui::step("Pass 1 — discovering project structure...");
    let (all_files, domains_map) = structure::discover_structure(&project_root)?;
    let total_files = all_files.len();

    let languages = structure::detect_languages(&all_files);
    ui::scan_progress(
        &format!(
            "{} files found, {} languages detected",
            total_files,
            languages.len()
        ),
        0.33,
    );

    if domains_map.is_empty() {
        ui::info("No domain candidates found. The wiki will start empty.");
        return Ok(ScanResult {
            domains: Vec::new(),
            total_files_scanned: total_files,
            languages_detected: languages,
        });
    }

    ui::step(&format!(
        "Found {} domain candidate(s): {}",
        domains_map.len(),
        domains_map.keys().cloned().collect::<Vec<_>>().join(", ")
    ));

    for (name, files) in &domains_map {
        ui::verbose(&format!("domain {:?} — {} file(s)", name, files.len()));
    }

    // Pass 2: Relationship analysis
    ui::step("Pass 2 — analyzing cross-domain dependencies...");
    let source_files: Vec<&PathBuf> = all_files
        .iter()
        .filter(|p| structure::is_source_file(p))
        .collect();

    let file_imports = imports::extract_all_imports(&source_files, &project_root);
    let dependency_graph =
        dependencies::build_dependency_graph(&domains_map, &file_imports, &project_root);
    ui::scan_progress(
        &format!("{} source files analyzed for imports", source_files.len()),
        0.66,
    );

    // Pass 3: Detail extraction
    ui::step("Pass 3 — extracting models, routes, and TODOs...");
    let mut domains: Vec<DomainInfo> = Vec::new();

    let domain_names: Vec<String> = domains_map.keys().cloned().collect();
    for (i, name) in domain_names.iter().enumerate() {
        let files = &domains_map[name];
        let extracted = details::extract_details(files, &project_root);

        let deps = dependency_graph.get(name).cloned().unwrap_or_default();

        let test_files: Vec<String> = files
            .iter()
            .filter(|f| structure::is_test_file(f))
            .map(|f| structure::relativize(f, &project_root))
            .collect();

        let relative_files: Vec<String> = files
            .iter()
            .map(|f| structure::relativize(f, &project_root))
            .collect();

        domains.push(DomainInfo {
            name: name.clone(),
            files: relative_files,
            dependencies: deps,
            models: extracted.models,
            routes: extracted.routes,
            comments: extracted.comments,
            test_files,
        });

        let progress = 0.66 + 0.34 * ((i + 1) as f64 / domain_names.len() as f64);
        ui::scan_progress(&format!("Extracted details for {}", name), progress);
    }

    // Sort domains alphabetically
    domains.sort_by(|a, b| a.name.cmp(&b.name));

    eprintln!();
    ui::success(&format!(
        "Scan complete: {} domains, {} files, {} languages",
        domains.len(),
        total_files,
        languages.len()
    ));

    Ok(ScanResult {
        domains,
        total_files_scanned: total_files,
        languages_detected: languages,
    })
}

// Re-export the DomainFileMap type for internal use
pub(crate) type DomainFileMap = HashMap<String, Vec<PathBuf>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_domain() -> DomainInfo {
        DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/mod.rs".to_string()],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }
    }

    #[test]
    fn has_signal_returns_false_for_files_only() {
        assert!(!empty_domain().has_signal());
    }

    #[test]
    fn has_signal_returns_true_with_models() {
        let mut d = empty_domain();
        d.models = vec!["Invoice".to_string()];
        assert!(d.has_signal());
    }

    #[test]
    fn has_signal_returns_true_with_routes() {
        let mut d = empty_domain();
        d.routes = vec!["GET /invoices".to_string()];
        assert!(d.has_signal());
    }

    #[test]
    fn has_signal_returns_true_with_dependencies() {
        let mut d = empty_domain();
        d.dependencies = vec!["users".to_string()];
        assert!(d.has_signal());
    }

    #[test]
    fn has_signal_returns_true_with_tests() {
        let mut d = empty_domain();
        d.test_files = vec!["tests/billing.test.ts".to_string()];
        assert!(d.has_signal());
    }

    #[test]
    fn has_signal_returns_true_with_comments() {
        let mut d = empty_domain();
        d.comments = vec!["[TODO] fix this".to_string()];
        assert!(d.has_signal());
    }

    #[test]
    fn structural_description_minimal() {
        let d = empty_domain();
        assert_eq!(
            d.structural_description(),
            "This domain contains 1 source file."
        );
    }

    #[test]
    fn structural_description_full() {
        let d = DomainInfo {
            name: "billing".to_string(),
            files: vec![
                "src/billing/mod.rs".to_string(),
                "src/billing/invoice.rs".to_string(),
                "tests/billing.test.rs".to_string(),
            ],
            dependencies: vec!["users".to_string()],
            models: vec!["Invoice".to_string(), "Payment".to_string()],
            routes: vec!["GET /invoices".to_string()],
            comments: vec![],
            test_files: vec!["tests/billing.test.rs".to_string()],
        };
        assert_eq!(
            d.structural_description(),
            "This domain contains 2 source files, 1 test file, 2 detected models, 1 API route, and 1 dependency on users."
        );
    }

    #[test]
    fn join_natural_formatting() {
        assert_eq!(join_natural(&[]), "");
        assert_eq!(join_natural(&["a".to_string()]), "a");
        assert_eq!(join_natural(&["a".to_string(), "b".to_string()]), "a and b");
        assert_eq!(
            join_natural(&["a".to_string(), "b".to_string(), "c".to_string()]),
            "a, b, and c"
        );
    }
}
