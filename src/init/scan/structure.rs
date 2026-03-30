use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use ignore::WalkBuilder;

use super::DomainFileMap;

// ─── Constants ───

/// Directories to skip that may not be covered by .gitignore.
/// The `ignore` crate already respects .gitignore and skips hidden dirs,
/// so we only need to add project-specific overrides here.
const EXTRA_SKIP_DIRS: &[&str] = &[
    ".wiki",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "vendor",
    "dist",
    "build",
    ".next",
];

pub(crate) const DOMAIN_PARENT_DIRS: &[&str] = &[
    "services",
    "modules",
    "features",
    "app",
    "lib",
    "packages",
    "controllers",
    "routes",
    "models",
    "api",
    "components",
    "handlers",
    "domains",
    "core",
    "plugins",
    "apps",
    // TypeScript frameworks
    "pages",      // Next.js pages router
    "middleware", // Express middleware
    "providers",  // NestJS providers
];

pub(crate) const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "rb", "php",
];

// ─── Pass 1: Structure Discovery ───

pub fn discover_structure(root: &Path) -> Result<(Vec<PathBuf>, DomainFileMap)> {
    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut domain_files: HashMap<String, Vec<PathBuf>> = HashMap::new();

    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true) // respect .gitignore
        .git_global(true) // respect global gitignore
        .git_exclude(true) // respect .git/info/exclude
        .follow_links(false);

    // Skip directories not covered by .gitignore (e.g. .wiki)
    walker.filter_entry(|entry| {
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            let name = entry.file_name().to_string_lossy();
            return !EXTRA_SKIP_DIRS.contains(&name.as_ref());
        }
        true
    });

    for entry in walker.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let Some(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }

        let path = entry.path().to_path_buf();
        all_files.push(path.clone());

        // Try to assign this file to a domain
        if let Some(domain_name) = extract_domain_name(&path, root) {
            domain_files.entry(domain_name).or_default().push(path);
        }
    }

    // Merge singular/plural domain duplicates (e.g., "user" + "users" → "users")
    merge_singular_plural_domains(&mut domain_files);

    // Also try to merge related files from different parent dirs into the same domain.
    // E.g. src/models/billing.ts should merge into the "billing" domain if it exists.
    merge_loose_files_into_domains(&mut domain_files, &all_files, root);

    Ok((all_files, domain_files))
}

pub fn extract_domain_name(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Look for patterns like: src/services/billing/... or packages/billing/...
    for (i, component) in components.iter().enumerate() {
        let lower = component.to_lowercase();
        if DOMAIN_PARENT_DIRS.contains(&lower.as_str()) {
            // The next component is the domain name — but skip Next.js route groups like (auth)
            let mut next_idx = i + 1;
            while next_idx < components.len() {
                let candidate = components[next_idx];
                // Next.js route groups: (group-name) — skip them
                if candidate.starts_with('(') && candidate.ends_with(')') {
                    next_idx += 1;
                    continue;
                }
                break;
            }

            if let Some(domain) = components.get(next_idx) {
                // Only if this next component is a directory (not the file itself),
                // or is a file that can be treated as a domain
                let domain_str = domain.to_string();
                // Strip all file extensions (handles billing.controller.ts → billing.controller)
                let domain_name = strip_all_extensions(&domain_str);

                // Skip if the "domain" looks like an index file
                if domain_name == "index"
                    || domain_name == "mod"
                    || domain_name == "__init__"
                    || domain_name == "main"
                    || domain_name == "page"
                    || domain_name == "layout"
                    || domain_name == "route"
                {
                    continue;
                }

                return Some(normalize_domain_name(&domain_name));
            }
        }
    }

    None
}

pub fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

pub fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let path_str = path.to_string_lossy();

    name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_test.")
        || name.starts_with("test_")
        || path_str.contains("/tests/")
        || path_str.contains("/test/")
        || path_str.contains("/__tests__/")
}

pub fn detect_languages(files: &[PathBuf]) -> Vec<String> {
    let mut langs: HashSet<String> = HashSet::new();

    for file in files {
        if let Some(ext) = file.extension().and_then(|e| e.to_str()) {
            let lang = match ext {
                "ts" | "tsx" => "TypeScript",
                "js" | "jsx" => "JavaScript",
                "py" => "Python",
                "rs" => "Rust",
                "go" => "Go",
                "java" => "Java",
                "rb" => "Ruby",
                "php" => "PHP",
                "css" | "scss" | "less" => "CSS",
                "html" | "htm" => "HTML",
                "json" => "JSON",
                "yaml" | "yml" => "YAML",
                "toml" => "TOML",
                "md" => "Markdown",
                "sql" => "SQL",
                "sh" | "bash" | "zsh" => "Shell",
                _ => continue,
            };
            langs.insert(lang.to_string());
        }
    }

    let mut sorted: Vec<String> = langs.into_iter().collect();
    sorted.sort();
    sorted
}

pub fn relativize(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

// ─── Domain name helpers ───

fn merge_singular_plural_domains(domain_files: &mut HashMap<String, Vec<PathBuf>>) {
    // Find pairs like ("user", "users") and merge the singular into the plural
    let keys: Vec<String> = domain_files.keys().cloned().collect();
    let mut merges: Vec<(String, String)> = Vec::new(); // (from, into)

    for key in &keys {
        // Check if singular form exists alongside plural
        let plural = format!("{}s", key);
        if keys.contains(&plural) {
            merges.push((key.clone(), plural));
        }
        // Also handle "y" → "ies" (e.g., "entity" → "entities")
        if key.ends_with("ies") {
            let singular = format!("{}y", &key[..key.len() - 3]);
            if keys.contains(&singular) {
                merges.push((singular, key.clone()));
            }
        }
    }

    for (from, into) in merges {
        if let Some(files) = domain_files.remove(&from) {
            domain_files.entry(into).or_default().extend(files);
        }
    }
}

fn merge_loose_files_into_domains(
    domain_files: &mut HashMap<String, Vec<PathBuf>>,
    all_files: &[PathBuf],
    root: &Path,
) {
    let existing_domains: HashSet<String> = domain_files.keys().cloned().collect();

    for file in all_files {
        if let Ok(rel) = file.strip_prefix(root) {
            // Get the file name without extensions and normalize it
            let raw_stem = file.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let normalized = normalize_domain_name(&strip_all_extensions(raw_stem));

            if normalized.is_empty() {
                continue;
            }

            // Try to find a matching domain (exact match or with 's' suffix)
            let matching_domain = if existing_domains.contains(&normalized) {
                Some(normalized.clone())
            } else if existing_domains.contains(&format!("{}s", normalized)) {
                Some(format!("{}s", normalized))
            } else if normalized.ends_with('s')
                && existing_domains.contains(&normalized[..normalized.len() - 1])
            {
                Some(normalized[..normalized.len() - 1].to_string())
            } else {
                None
            };

            if let Some(domain_name) = matching_domain {
                let files = domain_files.get_mut(&domain_name).unwrap();
                if !files.contains(file) {
                    let parent_name = rel
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("");

                    if DOMAIN_PARENT_DIRS.contains(&parent_name.to_lowercase().as_str()) {
                        files.push(file.clone());
                    }
                }
            }
        }
    }
}

pub(crate) fn normalize_domain_name(name: &str) -> String {
    let lower = name
        .to_lowercase()
        .replace(['/', '\\'], "")
        .replace("..", "");

    // Strip common suffixes like .controller, .service, .model, .module, .handler, .route, .test
    let suffixes = [
        ".controller",
        ".service",
        ".model",
        ".module",
        ".handler",
        ".route",
        ".routes",
        ".router",
        ".test",
        ".spec",
        ".dto",
        ".entity",
        ".repository",
        ".middleware",
        "-controller",
        "-service",
        "-model",
        "-module",
        "-handler",
        "-route",
        "-routes",
        "-router",
        "_controller",
        "_service",
        "_model",
        "_module",
        "_handler",
        "_route",
        "_routes",
        "_router",
    ];

    let mut cleaned = lower.as_str().to_string();
    for suffix in &suffixes {
        if let Some(stripped) = cleaned.strip_suffix(suffix) {
            cleaned = stripped.to_string();
            break;
        }
    }

    // Normalize separators
    cleaned = cleaned.replace(['_', ' '], "-");

    cleaned
}

pub(crate) fn strip_all_extensions(name: &str) -> String {
    // Strip file extensions progressively: billing.controller.ts → billing.controller → billing
    // Then normalize_domain_name handles the .controller suffix
    let mut result = name.to_string();
    while let Some(pos) = result.rfind('.') {
        let after = &result[pos + 1..];
        // If what's after the dot looks like an extension or a known suffix, strip it
        if SOURCE_EXTENSIONS.contains(&after) || after.len() <= 4 {
            result = result[..pos].to_string();
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ─── normalize_domain_name ───

    #[test]
    fn normalize_strips_controller_suffix() {
        assert_eq!(normalize_domain_name("billing.controller"), "billing");
    }

    #[test]
    fn normalize_strips_service_suffix_with_hyphen() {
        assert_eq!(normalize_domain_name("user-service"), "user");
    }

    #[test]
    fn normalize_strips_handler_suffix_with_underscore() {
        assert_eq!(normalize_domain_name("auth_handler"), "auth");
    }

    #[test]
    fn normalize_leaves_simple_name_unchanged() {
        assert_eq!(normalize_domain_name("simple"), "simple");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_domain_name("Billing"), "billing");
    }

    #[test]
    fn normalize_replaces_underscores_with_hyphens() {
        assert_eq!(normalize_domain_name("my_domain"), "my-domain");
    }

    // ─── strip_all_extensions ───

    #[test]
    fn strip_extensions_ts_file() {
        assert_eq!(
            strip_all_extensions("billing.controller.ts"),
            "billing.controller"
        );
    }

    #[test]
    fn strip_extensions_simple_ts() {
        assert_eq!(strip_all_extensions("index.ts"), "index");
    }

    #[test]
    fn strip_extensions_no_extension() {
        assert_eq!(strip_all_extensions("simple"), "simple");
    }

    #[test]
    fn strip_extensions_multiple_extensions() {
        assert_eq!(strip_all_extensions("foo.spec.test.ts"), "foo");
    }

    // ─── discover_structure ───

    #[test]
    fn discover_structure_finds_domain_under_services() {
        let dir = TempDir::new().unwrap();
        let billing_dir = dir.path().join("src/services/billing");
        fs::create_dir_all(&billing_dir).unwrap();
        fs::write(billing_dir.join("invoice.ts"), "export class Invoice {}").unwrap();

        let (files, domains) = discover_structure(dir.path()).unwrap();

        assert!(!files.is_empty());
        assert!(
            domains.contains_key("billing"),
            "Expected 'billing' domain, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn discover_structure_finds_domain_under_modules() {
        let dir = TempDir::new().unwrap();
        let auth_dir = dir.path().join("src/modules/auth");
        fs::create_dir_all(&auth_dir).unwrap();
        fs::write(auth_dir.join("login.ts"), "export function login() {}").unwrap();

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("auth"),
            "Expected 'auth' domain, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn discover_structure_skips_node_modules() {
        let dir = TempDir::new().unwrap();
        let nm_dir = dir.path().join("node_modules/some-package");
        fs::create_dir_all(&nm_dir).unwrap();
        fs::write(nm_dir.join("index.js"), "module.exports = {}").unwrap();

        let (files, _domains) = discover_structure(dir.path()).unwrap();

        // No files from node_modules should appear
        for f in &files {
            assert!(
                !f.to_string_lossy().contains("node_modules"),
                "node_modules file should be skipped: {:?}",
                f
            );
        }
    }

    // ─── TypeScript framework domain detection ───

    #[test]
    fn nextjs_app_router_detects_domain() {
        let dir = TempDir::new().unwrap();
        let billing_dir = dir.path().join("app/billing");
        fs::create_dir_all(&billing_dir).unwrap();
        fs::write(
            billing_dir.join("page.tsx"),
            "export default function Page() {}",
        )
        .unwrap();

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("billing"),
            "Expected 'billing' domain from Next.js app router, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn nextjs_app_router_skips_route_groups() {
        let dir = TempDir::new().unwrap();
        let billing_dir = dir.path().join("app/(dashboard)/billing");
        fs::create_dir_all(&billing_dir).unwrap();
        fs::write(
            billing_dir.join("page.tsx"),
            "export default function Page() {}",
        )
        .unwrap();

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("billing"),
            "Expected 'billing' domain (skipping route group), found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn nextjs_pages_router_detects_domain() {
        let dir = TempDir::new().unwrap();
        let billing_dir = dir.path().join("pages/billing");
        fs::create_dir_all(&billing_dir).unwrap();
        fs::write(
            billing_dir.join("index.tsx"),
            "export default function Page() {}",
        )
        .unwrap();

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("billing"),
            "Expected 'billing' domain from Next.js pages router, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn monorepo_packages_detects_domain() {
        let dir = TempDir::new().unwrap();
        let billing_dir = dir.path().join("packages/billing/src");
        fs::create_dir_all(&billing_dir).unwrap();
        fs::write(billing_dir.join("index.ts"), "export class Invoice {}").unwrap();

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("billing"),
            "Expected 'billing' domain from monorepo packages, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    // ─── merge_singular_plural_domains ───

    #[test]
    fn merge_singular_into_plural() {
        let mut domains: HashMap<String, Vec<PathBuf>> = HashMap::new();
        domains.insert("user".to_string(), vec![PathBuf::from("a.ts")]);
        domains.insert("users".to_string(), vec![PathBuf::from("b.ts")]);

        merge_singular_plural_domains(&mut domains);

        assert!(!domains.contains_key("user"));
        assert!(domains.contains_key("users"));
        assert_eq!(domains["users"].len(), 2);
    }

    #[test]
    fn merge_does_not_merge_unrelated() {
        let mut domains: HashMap<String, Vec<PathBuf>> = HashMap::new();
        domains.insert("billing".to_string(), vec![PathBuf::from("a.ts")]);
        domains.insert("auth".to_string(), vec![PathBuf::from("b.ts")]);

        merge_singular_plural_domains(&mut domains);

        assert!(domains.contains_key("billing"));
        assert!(domains.contains_key("auth"));
    }
}
