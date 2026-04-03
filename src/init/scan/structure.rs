use std::collections::{HashMap, HashSet};
use std::fs;
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

/// Top-level directories that are infrastructure, not business domains.
/// These are excluded from app-directory detection.
const INFRA_DIRS: &[&str] = &[
    "src",
    "config",
    "scripts",
    "deploy",
    "static",
    "templates",
    "public",
    "assets",
    "docs",
    "doc",
    "test",
    "tests",
    "spec",
    "bin",
    "build",
    "dist",
    "media",
    "locale",
    "fixtures",
    "migrations",
    "management",
    "node_modules",
    "vendor",
    ".github",
    ".vscode",
    ".idea",
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

    // Pre-scan: detect top-level directories that look like app modules
    // (e.g. Django apps, Go packages, standalone Python packages).
    // These take priority over DOMAIN_PARENT_DIRS to avoid misdetecting
    // `api/views.py` as domain "views" instead of domain "api".
    let app_dirs = detect_app_directories(root);

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

        // Try app-directory detection first (accounts/views.py → "accounts").
        // This must run before DOMAIN_PARENT_DIRS to avoid "api" being treated
        // as a parent dir and extracting "views" as the domain.
        if let Some(domain_name) = extract_app_domain(&path, root, &app_dirs) {
            domain_files.entry(domain_name).or_default().push(path);
        } else if let Some(domain_name) = extract_domain_name(&path, root) {
            domain_files.entry(domain_name).or_default().push(path);
        }
    }

    // Resolve naming conflicts between sub-domains and other domains.
    // E.g. if both a top-level `utils/` and `django/utils/` produce domain "utils",
    // rename the sub-domain to "django-utils".
    resolve_subdomain_conflicts(&mut domain_files, &app_dirs, root);

    // Merge singular/plural domain duplicates (e.g., "user" + "users" → "users")
    merge_singular_plural_domains(&mut domain_files);

    // Also try to merge related files from different parent dirs into the same domain.
    // E.g. src/models/billing.ts should merge into the "billing" domain if it exists.
    merge_loose_files_into_domains(&mut domain_files, &all_files, root);

    Ok((all_files, domain_files))
}

/// Minimum number of sub-packages required before considering splitting.
const LARGE_APP_SUBPACKAGE_THRESHOLD: usize = 4;

/// Minimum total source files (recursive) in the app directory before splitting.
/// Below this, the directory stays as a single domain even with many sub-packages.
/// Rationale: LLM analysis samples ~10 files per domain. At 30+ files, a single
/// domain would only cover ~33% of the codebase, so splitting improves quality.
const LARGE_APP_FILE_THRESHOLD: usize = 30;

/// Info about a detected top-level app directory.
struct AppDirInfo {
    /// If the directory is "large", maps sub-package names to their paths.
    /// When empty, the directory is treated as a single domain (small app).
    sub_domains: HashMap<String, PathBuf>,
}

/// Detect top-level directories that look like app modules.
///
/// A directory is considered an app if it:
/// - Contains `__init__.py` (Python package — Django/Flask app)
/// - OR contains ≥3 direct source files
///
/// Large app directories (≥ LARGE_APP_SUBPACKAGE_THRESHOLD sub-packages) are
/// automatically split into sub-domains. For example, `django/` with sub-packages
/// `forms/`, `db/`, `middleware/` etc. produces separate domains for each.
///
/// Infrastructure directories (src, config, scripts, etc.) are excluded.
fn detect_app_directories(root: &Path) -> HashMap<String, AppDirInfo> {
    let mut app_dirs = HashMap::new();

    let Ok(entries) = fs::read_dir(root) else {
        return app_dirs;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden dirs
        if name.starts_with('.') {
            continue;
        }

        // Skip known infrastructure directories
        if INFRA_DIRS.contains(&name.to_lowercase().as_str()) {
            continue;
        }

        // Skip directories in EXTRA_SKIP_DIRS
        if EXTRA_SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        // Check for app markers
        let has_init_py = path.join("__init__.py").exists();
        let has_enough_source_files = count_direct_source_files(&path) >= 3;

        if has_init_py || has_enough_source_files {
            let sub_packages = detect_sub_packages(&path);
            let total_source_files = count_recursive_source_files(&path);
            let should_split = sub_packages.len() >= LARGE_APP_SUBPACKAGE_THRESHOLD
                && total_source_files >= LARGE_APP_FILE_THRESHOLD;

            app_dirs.insert(
                normalize_domain_name(&name),
                AppDirInfo {
                    sub_domains: if should_split {
                        sub_packages
                    } else {
                        HashMap::new()
                    },
                },
            );
        }
    }

    app_dirs
}

/// Detect sub-directories within an app directory that qualify as sub-packages.
///
/// A sub-directory qualifies if it contains `__init__.py` or ≥3 source files.
/// Infrastructure and skip directories are excluded.
fn detect_sub_packages(app_dir: &Path) -> HashMap<String, PathBuf> {
    let mut sub_packages = HashMap::new();

    let Ok(entries) = fs::read_dir(app_dir) else {
        return sub_packages;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if INFRA_DIRS.contains(&name.to_lowercase().as_str()) {
            continue;
        }

        if EXTRA_SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        let has_init_py = path.join("__init__.py").exists();
        let has_enough_source_files = count_direct_source_files(&path) >= 3;

        if has_init_py || has_enough_source_files {
            sub_packages.insert(normalize_domain_name(&name), path);
        }
    }

    sub_packages
}

/// Count source files recursively inside a directory.
/// Used to determine if an app directory has enough code mass to justify splitting.
fn count_recursive_source_files(dir: &Path) -> usize {
    let mut count = 0;

    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += count_recursive_source_files(&path);
        } else if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
                .unwrap_or(false)
        {
            count += 1;
        }
    }

    count
}

/// Count source files directly inside a directory (not recursive).
fn count_direct_source_files(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    entries
        .flatten()
        .filter(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
                    .unwrap_or(false)
        })
        .count()
}

/// Extract domain from a file path using pre-detected app directories.
///
/// If the first path component (relative to root) matches a known app directory,
/// use that as the domain name. For large app directories that have been split
/// into sub-domains, the second path component determines the domain name.
///
/// Examples:
/// - Small app: `accounts/views.py` → domain "accounts"
/// - Large app: `django/forms/fields.py` → domain "forms"
/// - Large app root file: `django/__init__.py` → domain "django"
fn extract_app_domain(
    path: &Path,
    root: &Path,
    app_dirs: &HashMap<String, AppDirInfo>,
) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    let first_component = components.first()?;
    let normalized_first = normalize_domain_name(first_component);

    let app_info = app_dirs.get(&normalized_first)?;

    // Large app directory with sub-domains: try to assign to a sub-domain.
    if !app_info.sub_domains.is_empty() {
        if let Some(second_component) = components.get(1) {
            let normalized_second = normalize_domain_name(second_component);
            if app_info.sub_domains.contains_key(&normalized_second) {
                return Some(normalized_second);
            }
        }
        // File is directly in the large app dir root (e.g. django/__init__.py)
        // or in a sub-directory that isn't a recognized sub-package.
        // Fall back to the parent domain name.
        return Some(normalized_first);
    }

    // Small app directory: use as single domain (current behavior).
    Some(normalized_first)
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

/// Resolve naming conflicts between sub-domains of large app dirs and other domains.
///
/// When a large app dir like `django/` is split, its sub-package `django/utils/` produces
/// a domain named "utils". If a separate top-level `utils/` app also exists, we have a
/// conflict. This function detects such cases and renames the sub-domain to "django-utils".
fn resolve_subdomain_conflicts(
    domain_files: &mut HashMap<String, Vec<PathBuf>>,
    app_dirs: &HashMap<String, AppDirInfo>,
    root: &Path,
) {
    // Build a map: sub-domain name → parent app dir name
    let mut sub_domain_parents: HashMap<String, String> = HashMap::new();
    for (parent_name, info) in app_dirs {
        for sub_name in info.sub_domains.keys() {
            sub_domain_parents.insert(sub_name.clone(), parent_name.clone());
        }
    }

    let domain_names: Vec<String> = domain_files.keys().cloned().collect();
    let mut renames: Vec<(String, String, Vec<PathBuf>)> = Vec::new(); // (old_name, new_name, files_to_move)

    for name in &domain_names {
        let Some(parent_name) = sub_domain_parents.get(name) else {
            continue;
        };

        let files = &domain_files[name];
        let parent_prefix = root.join(parent_name);

        let has_external = files.iter().any(|f| !f.starts_with(&parent_prefix));
        let has_internal = files.iter().any(|f| f.starts_with(&parent_prefix));

        if has_external && has_internal {
            // Conflict: files from both the sub-domain and another source share a name.
            // Move the internal (sub-domain) files to a qualified name.
            let internal_files: Vec<PathBuf> = files
                .iter()
                .filter(|f| f.starts_with(&parent_prefix))
                .cloned()
                .collect();

            let qualified_name = format!("{}-{}", parent_name, name);
            renames.push((name.clone(), qualified_name, internal_files));
        }
    }

    for (old_name, new_name, internal_files) in renames {
        // Remove internal files from the old domain
        if let Some(files) = domain_files.get_mut(&old_name) {
            files.retain(|f| !internal_files.contains(f));
            if files.is_empty() {
                domain_files.remove(&old_name);
            }
        }
        // Add them under the qualified name
        domain_files
            .entry(new_name)
            .or_default()
            .extend(internal_files);
    }
}

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

    // ─── Large app directory splitting ───

    /// Helper: create a Python sub-package (directory with __init__.py)
    /// with enough files to be realistic.
    fn create_python_package(parent: &Path, name: &str) {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("__init__.py"), "").unwrap();
        fs::write(dir.join("models.py"), "class Foo: pass").unwrap();
    }

    /// Helper: create a rich Python sub-package with multiple source files
    /// to ensure the parent directory exceeds LARGE_APP_FILE_THRESHOLD.
    fn create_rich_python_package(parent: &Path, name: &str) {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("__init__.py"), "").unwrap();
        for i in 0..6 {
            fs::write(
                dir.join(format!("module_{}.py", i)),
                format!("class Mod{}: pass", i),
            )
            .unwrap();
        }
    }

    #[test]
    fn large_app_dir_splits_into_subdomains() {
        let dir = TempDir::new().unwrap();
        let app = dir.path().join("myframework");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("__init__.py"), "").unwrap();

        // Create 5 rich sub-packages: 5 * 7 files = 35 (above file threshold of 30)
        for name in &["forms", "db", "middleware", "http", "views"] {
            create_rich_python_package(&app, name);
        }

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        for sub in &["forms", "db", "middleware", "http", "views"] {
            assert!(
                domains.contains_key(*sub),
                "Expected sub-domain '{}', found: {:?}",
                sub,
                domains.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn small_app_dir_stays_single_domain() {
        let dir = TempDir::new().unwrap();
        let app = dir.path().join("myapp");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("__init__.py"), "").unwrap();
        fs::write(app.join("views.py"), "def index(): pass").unwrap();

        // Create only 2 sub-packages (below sub-package threshold of 4)
        create_python_package(&app, "models");
        create_python_package(&app, "utils");

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        assert!(
            domains.contains_key("myapp"),
            "Expected single 'myapp' domain, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn app_with_many_subpackages_but_few_files_stays_single() {
        let dir = TempDir::new().unwrap();
        let app = dir.path().join("myapp");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("__init__.py"), "").unwrap();

        // Create 5 sub-packages but each with only 2 files → 10 total (below 30)
        for name in &["forms", "db", "middleware", "http", "views"] {
            create_python_package(&app, name);
        }

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        // Should stay as a single domain because total files < 30
        assert!(
            domains.contains_key("myapp"),
            "Expected single 'myapp' domain (not enough files to split), found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
        assert!(
            !domains.contains_key("forms"),
            "Should NOT split with only ~10 source files total"
        );
    }

    #[test]
    fn large_app_root_files_fallback_to_parent_domain() {
        let dir = TempDir::new().unwrap();
        let app = dir.path().join("myframework");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("__init__.py"), "# root init").unwrap();
        fs::write(app.join("shortcuts.py"), "def redirect(): pass").unwrap();

        // Create 5 rich sub-packages to trigger splitting
        for name in &["forms", "db", "middleware", "http", "views"] {
            create_rich_python_package(&app, name);
        }

        let (_files, domains) = discover_structure(dir.path()).unwrap();

        // shortcuts.py is in the root of the large app, should go to "myframework"
        assert!(
            domains.contains_key("myframework"),
            "Expected 'myframework' domain for root files, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn detect_sub_packages_finds_python_packages() {
        let dir = TempDir::new().unwrap();
        let app = dir.path();

        create_python_package(app, "forms");
        create_python_package(app, "db");
        // Create a non-package dir (no __init__.py, < 3 source files)
        fs::create_dir_all(app.join("data")).unwrap();
        fs::write(app.join("data/readme.txt"), "not a package").unwrap();

        let sub_pkgs = detect_sub_packages(app);

        assert!(sub_pkgs.contains_key("forms"));
        assert!(sub_pkgs.contains_key("db"));
        assert!(!sub_pkgs.contains_key("data"));
    }

    #[test]
    fn detect_sub_packages_skips_infra_dirs() {
        let dir = TempDir::new().unwrap();
        let app = dir.path();

        create_python_package(app, "forms");
        // "test" is in INFRA_DIRS and should be skipped
        create_python_package(app, "test");

        let sub_pkgs = detect_sub_packages(app);

        assert!(sub_pkgs.contains_key("forms"));
        assert!(
            !sub_pkgs.contains_key("test"),
            "Infra dir 'test' should be excluded from sub-packages"
        );
    }

    #[test]
    fn subdomain_conflict_resolution() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create a large app with a "utils" sub-package (rich enough to trigger split)
        let app = root.join("myframework");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("__init__.py"), "").unwrap();
        for name in &["forms", "db", "middleware", "http", "utils"] {
            create_rich_python_package(&app, name);
        }

        // Also create a top-level "utils" app dir
        let top_utils = root.join("utils");
        fs::create_dir_all(&top_utils).unwrap();
        fs::write(top_utils.join("__init__.py"), "").unwrap();
        fs::write(top_utils.join("helpers.py"), "def help(): pass").unwrap();
        fs::write(top_utils.join("strings.py"), "def strip(): pass").unwrap();
        fs::write(top_utils.join("dates.py"), "def now(): pass").unwrap();

        let (_files, domains) = discover_structure(root).unwrap();

        // One of them should be renamed to avoid conflict
        let has_utils = domains.contains_key("utils");
        let has_qualified = domains.contains_key("myframework-utils");

        assert!(
            has_utils && has_qualified,
            "Expected both 'utils' and 'myframework-utils', found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }
}
