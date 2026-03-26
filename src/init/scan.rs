use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use rayon::prelude::*;
use regex::Regex;
use ignore::WalkBuilder;

// ─── Pre-compiled regex patterns ───

// JS/TS imports
static RE_JS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"import\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap()
});
static RE_JS_REQUIRE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"require\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
});
static RE_JS_EXPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"export\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap()
});

// Python imports
static RE_PY_FROM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^from\s+(\S+)\s+import").unwrap()
});
static RE_PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^import\s+(\S+)").unwrap()
});

// Rust imports
static RE_RS_USE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"use\s+crate::(\S+?)(?:::\{|;)").unwrap()
});
static RE_RS_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:pub\s+)?mod\s+(\w+)\s*;").unwrap()
});

// Go imports
static RE_GO_SINGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"import\s+"([^"]+)""#).unwrap()
});
static RE_GO_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"import\s*\(([\s\S]*?)\)"#).unwrap()
});
static RE_GO_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""([^"]+)""#).unwrap()
});

// Comments (TODO/FIXME/HACK/NOTE)
static RE_COMMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?://|#|/\*)\s*(TODO|FIXME|HACK|NOTE)\b[:\s]*(.*)").unwrap()
});

// Model/type definitions per language
static RE_JS_MODELS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:export\s+)?(?:interface|type|class|enum)\s+(\w+)").unwrap()
});
static RE_PY_CLASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"class\s+(\w+)").unwrap()
});
static RE_RS_STRUCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:pub\s+)?(?:struct|enum|trait)\s+(\w+)").unwrap()
});
static RE_GO_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"type\s+(\w+)\s+struct").unwrap()
});

// Route/endpoint extraction
static RE_EXPRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:app|router)\.\s*(get|post|put|patch|delete)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
});
static RE_FLASK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@\w+\.(?:route|get|post|put|patch|delete)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
});
static RE_NEXTJS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"export\s+(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE)").unwrap()
});
static RE_ACTIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"#\[\s*(get|post|put|patch|delete)\s*\(\s*"([^"]+)""#).unwrap()
});
static RE_GO_HTTP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:HandleFunc|Handle)\s*\(\s*"([^"]+)""#).unwrap()
});

use crate::ui;
use crate::wiki::common::capitalize;

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

const DOMAIN_PARENT_DIRS: &[&str] = &[
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
];

const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "rb", "php",
];

// ─── Main entry point ───

pub fn run() -> Result<ScanResult> {
    let project_root = std::env::current_dir().context("Failed to get current directory")?;

    ui::action("Scanning codebase");
    eprintln!();

    // Pass 1: Structure discovery
    ui::step("Pass 1 — discovering project structure...");
    let (all_files, domains_map) = discover_structure(&project_root)?;
    let total_files = all_files.len();

    let languages = detect_languages(&all_files);
    ui::scan_progress(
        &format!("{} files found, {} languages detected", total_files, languages.len()),
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
        domains_map
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    ));

    for (name, files) in &domains_map {
        ui::verbose(&format!("domain {:?} — {} file(s)", name, files.len()));
    }

    // Pass 2: Relationship analysis
    ui::step("Pass 2 — analyzing cross-domain dependencies...");
    let source_files: Vec<&PathBuf> = all_files
        .iter()
        .filter(|p| is_source_file(p))
        .collect();

    let imports = extract_all_imports(&source_files, &project_root);
    let dependency_graph = build_dependency_graph(&domains_map, &imports, &project_root);
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
        let details = extract_details(files, &project_root);

        let dependencies = dependency_graph
            .get(name)
            .cloned()
            .unwrap_or_default();

        let test_files: Vec<String> = files
            .iter()
            .filter(|f| is_test_file(f))
            .map(|f| relativize(f, &project_root))
            .collect();

        let relative_files: Vec<String> = files
            .iter()
            .map(|f| relativize(f, &project_root))
            .collect();

        domains.push(DomainInfo {
            name: name.clone(),
            files: relative_files,
            dependencies,
            models: details.models,
            routes: details.routes,
            comments: details.comments,
            test_files,
        });

        let progress = 0.66 + 0.34 * ((i + 1) as f64 / domain_names.len() as f64);
        ui::scan_progress(
            &format!("Extracted details for {}", name),
            progress,
        );
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

// ─── Pass 1: Structure Discovery ───

fn discover_structure(root: &Path) -> Result<(Vec<PathBuf>, HashMap<String, Vec<PathBuf>>)> {
    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut domain_files: HashMap<String, Vec<PathBuf>> = HashMap::new();

    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(true)           // skip hidden files/dirs
        .git_ignore(true)       // respect .gitignore
        .git_global(true)       // respect global gitignore
        .git_exclude(true)      // respect .git/info/exclude
        .follow_links(false);

    // Skip directories not covered by .gitignore (e.g. .wiki)
    walker.filter_entry(|entry| {
        if entry.file_type().map_or(false, |ft| ft.is_dir()) {
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
            domain_files
                .entry(domain_name)
                .or_default()
                .push(path);
        }
    }

    // Merge singular/plural domain duplicates (e.g., "user" + "users" → "users")
    merge_singular_plural_domains(&mut domain_files);

    // Also try to merge related files from different parent dirs into the same domain.
    // E.g. src/models/billing.ts should merge into the "billing" domain if it exists.
    merge_loose_files_into_domains(&mut domain_files, &all_files, root);

    Ok((all_files, domain_files))
}

fn extract_domain_name(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Look for patterns like: src/services/billing/... or packages/billing/...
    for (i, component) in components.iter().enumerate() {
        let lower = component.to_lowercase();
        if DOMAIN_PARENT_DIRS.contains(&lower.as_str()) {
            // The next component is the domain name
            if let Some(domain) = components.get(i + 1) {
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
                {
                    continue;
                }

                return Some(normalize_domain_name(&domain_name));
            }
        }
    }

    None
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
            let raw_stem = file
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let normalized = normalize_domain_name(&strip_all_extensions(raw_stem));

            if normalized.is_empty() {
                continue;
            }

            // Try to find a matching domain (exact match or with 's' suffix)
            let matching_domain = if existing_domains.contains(&normalized) {
                Some(normalized.clone())
            } else if existing_domains.contains(&format!("{}s", normalized)) {
                Some(format!("{}s", normalized))
            } else if normalized.ends_with('s') && existing_domains.contains(&normalized[..normalized.len()-1]) {
                Some(normalized[..normalized.len()-1].to_string())
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

fn normalize_domain_name(name: &str) -> String {
    let lower = name.to_lowercase()
        .replace('/', "")
        .replace('\\', "")
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
    cleaned = cleaned.replace('_', "-").replace(' ', "-");

    // Singularize simple plurals for merging (users → user is NOT desired,
    // we keep the original form but normalize known patterns)
    cleaned
}

fn strip_all_extensions(name: &str) -> String {
    // Strip file extensions progressively: billing.controller.ts → billing.controller → billing
    // Then normalize_domain_name handles the .controller suffix
    let mut result = name.to_string();
    loop {
        match result.rfind('.') {
            Some(pos) => {
                let after = &result[pos + 1..];
                // If what's after the dot looks like an extension or a known suffix, strip it
                if SOURCE_EXTENSIONS.contains(&after) || after.len() <= 4 {
                    result = result[..pos].to_string();
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    result
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let path_str = path.to_string_lossy();

    name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_test.")
        || name.starts_with("test_")
        || path_str.contains("/tests/")
        || path_str.contains("/test/")
        || path_str.contains("/__tests__/")
}

fn detect_languages(files: &[PathBuf]) -> Vec<String> {
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

fn relativize(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

// ─── Pass 2: Relationship Analysis ───

#[derive(Debug, Default)]
struct FileImports {
    file_path: PathBuf,
    imports: Vec<String>,
}

fn extract_all_imports(files: &[&PathBuf], _root: &Path) -> Vec<FileImports> {
    files
        .par_iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            let imports = extract_imports(path, &content);
            if imports.is_empty() {
                None
            } else {
                Some(FileImports {
                    file_path: path.to_path_buf(),
                    imports,
                })
            }
        })
        .collect()
}

fn extract_imports(path: &Path, content: &str) -> Vec<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "ts" | "tsx" | "js" | "jsx" => extract_js_imports(content),
        "py" => extract_python_imports(content),
        "rs" => extract_rust_imports(content),
        "go" => extract_go_imports(content),
        _ => Vec::new(),
    }
}

fn extract_js_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();

    // import ... from '...'
    for cap in RE_JS_IMPORT.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    // require('...')
    for cap in RE_JS_REQUIRE.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    // export ... from '...'
    for cap in RE_JS_EXPORT.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    imports
}

fn extract_python_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();

    // from X import Y
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(cap) = RE_PY_FROM.captures(trimmed) {
            imports.push(cap[1].to_string());
        }
    }

    // import X
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") && !trimmed.contains(" from ") {
            if let Some(cap) = RE_PY_IMPORT.captures(trimmed) {
                imports.push(cap[1].to_string());
            }
        }
    }

    imports
}

fn extract_rust_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();

    // use crate::something
    for cap in RE_RS_USE.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    // mod something
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(cap) = RE_RS_MOD.captures(trimmed) {
            imports.push(cap[1].to_string());
        }
    }

    imports
}

fn extract_go_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();

    // Single import: import "path"
    for cap in RE_GO_SINGLE.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    // Multi-line import block: import ( "path1" "path2" )
    for block_cap in RE_GO_BLOCK.captures_iter(content) {
        for path_cap in RE_GO_PATH.captures_iter(&block_cap[1]) {
            imports.push(path_cap[1].to_string());
        }
    }

    imports
}

fn build_dependency_graph(
    domains: &HashMap<String, Vec<PathBuf>>,
    imports: &[FileImports],
    root: &Path,
) -> HashMap<String, Vec<String>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    // Build a lookup: domain name -> set of path fragments that identify this domain
    let mut domain_identifiers: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, files) in domains {
        let mut idents = HashSet::new();
        idents.insert(name.clone());
        idents.insert(name.replace('-', "_"));

        for file in files {
            if let Ok(rel) = file.strip_prefix(root) {
                // Add the relative path so we can match imports against it
                let rel_str = rel.to_string_lossy().to_string();
                idents.insert(rel_str);

                // Also add without extension
                if let Some(stem) = rel.with_extension("").to_str() {
                    idents.insert(stem.to_string());
                }
            }
        }

        domain_identifiers.insert(name.clone(), idents);
    }

    // For each file with imports, find which domain it belongs to,
    // then check if its imports point to other domains
    for fi in imports {
        let source_domain = match extract_domain_name(&fi.file_path, root) {
            Some(d) => d,
            None => continue,
        };

        if !domains.contains_key(&source_domain) {
            continue;
        }

        for import_path in &fi.imports {
            let import_lower = import_path.to_lowercase().replace('_', "-");

            for (target_domain, idents) in &domain_identifiers {
                if *target_domain == source_domain {
                    continue;
                }

                // Check if any identifier matches a portion of the import path
                let matches = idents.iter().any(|ident| {
                    let ident_lower = ident.to_lowercase().replace('_', "-");
                    import_lower.contains(&ident_lower)
                        || ident_lower.contains(&import_lower)
                });

                if matches {
                    let deps = graph.entry(source_domain.clone()).or_default();
                    if !deps.contains(target_domain) {
                        deps.push(target_domain.clone());
                    }
                }
            }
        }
    }

    // Sort dependency lists
    for deps in graph.values_mut() {
        deps.sort();
    }

    graph
}

// ─── Pass 3: Detail Extraction ───

#[derive(Debug, Default)]
struct DomainDetails {
    models: Vec<String>,
    routes: Vec<String>,
    comments: Vec<String>,
}

fn extract_details(files: &[PathBuf], _root: &Path) -> DomainDetails {
    let results: Vec<DomainDetails> = files
        .par_iter()
        .filter_map(|path| {
            if !is_source_file(path) {
                return None;
            }
            let content = fs::read_to_string(path).ok()?;
            Some(extract_file_details(&content, path))
        })
        .collect();

    let mut merged = DomainDetails::default();
    for r in results {
        merged.models.extend(r.models);
        merged.routes.extend(r.routes);
        merged.comments.extend(r.comments);
    }

    // Deduplicate
    merged.models.sort();
    merged.models.dedup();
    merged.routes.sort();
    merged.routes.dedup();

    merged
}

fn extract_file_details(content: &str, path: &Path) -> DomainDetails {
    let mut details = DomainDetails::default();

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Extract TODO/FIXME/HACK/NOTE comments
    for cap in RE_COMMENTS.captures_iter(content) {
        let tag = &cap[1];
        let text = cap[2].trim().trim_end_matches("*/").trim();
        if !text.is_empty() {
            details
                .comments
                .push(format!("[{}] {}", tag, text));
        }
    }

    // Extract model/type/struct/class/interface definitions
    match ext {
        "ts" | "tsx" | "js" | "jsx" => {
            for cap in RE_JS_MODELS.captures_iter(content) {
                details.models.push(cap[1].to_string());
            }
        }
        "py" => {
            for cap in RE_PY_CLASS.captures_iter(content) {
                details.models.push(cap[1].to_string());
            }
        }
        "rs" => {
            for cap in RE_RS_STRUCT.captures_iter(content) {
                details.models.push(cap[1].to_string());
            }
        }
        "go" => {
            for cap in RE_GO_TYPE.captures_iter(content) {
                details.models.push(cap[1].to_string());
            }
        }
        _ => {}
    }

    // Extract route/endpoint definitions
    // Express-style: app.get('/...'), router.post('/...')
    for cap in RE_EXPRESS.captures_iter(content) {
        details.routes.push(format!(
            "{} {}",
            cap[1].to_uppercase(),
            &cap[2]
        ));
    }

    // Python/Flask/FastAPI decorators: @app.route('/...'), @router.get('/...')
    for cap in RE_FLASK.captures_iter(content) {
        details.routes.push(cap[1].to_string());
    }

    // Next.js API routes (infer from file path pattern)
    let path_str = path.to_string_lossy();
    if path_str.contains("/api/") && (ext == "ts" || ext == "js" || ext == "tsx" || ext == "jsx") {
        // Check for HTTP method exports: export async function GET/POST/etc.
        for cap in RE_NEXTJS.captures_iter(content) {
            if let Some(route) = extract_nextjs_route(path) {
                details
                    .routes
                    .push(format!("{} {}", &cap[1], route));
            }
        }
    }

    // Rust Actix/Axum style: #[get("/...")]
    for cap in RE_ACTIX.captures_iter(content) {
        details.routes.push(format!(
            "{} {}",
            cap[1].to_uppercase(),
            &cap[2]
        ));
    }

    // Go: http.HandleFunc("/...", handler)
    for cap in RE_GO_HTTP.captures_iter(content) {
        details.routes.push(cap[1].to_string());
    }

    details
}

fn extract_nextjs_route(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    // Find the /api/ segment and build route from it
    if let Some(idx) = path_str.find("/api/") {
        let route_part = &path_str[idx..];
        // Remove file extension and route.ts/route.js
        let route = route_part
            .trim_end_matches(".ts")
            .trim_end_matches(".tsx")
            .trim_end_matches(".js")
            .trim_end_matches(".jsx")
            .trim_end_matches("/route")
            .trim_end_matches("/index");
        return Some(route.to_string());
    }
    None
}

// ─── Wiki Generation ───

pub fn generate_domain_overview(domain: &DomainInfo, all_domains: &[DomainInfo]) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let title = domain
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string() + &domain.name[1..])
        .unwrap_or_default();

    let related_files_yaml: String = domain
        .files
        .iter()
        .map(|f| format!("  - {}", f))
        .collect::<Vec<_>>()
        .join("\n");

    let related_files_section = if related_files_yaml.is_empty() {
        "related_files: []".to_string()
    } else {
        format!("related_files:\n{}", related_files_yaml)
    };

    let mut sections = Vec::new();

    // Front matter
    sections.push(format!(
        "---\ndomain: {}\nconfidence: inferred\nlast_updated: {}\n{}\n---",
        domain.name, date, related_files_section
    ));

    // Title and description
    sections.push(format!(
        "# {}\n\n## Description\n_Auto-generated from codebase scan. Needs human review._ `[inferred]`",
        title
    ));

    // Key behaviors
    sections.push("## Key behaviors\n_To be documented._".to_string());

    // Data models
    if domain.models.is_empty() {
        sections.push("## Data models\n_None detected._".to_string());
    } else {
        let models_list: String = domain
            .models
            .iter()
            .map(|m| format!("- {} `[seen-in-code]`", m))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Data models\n{}", models_list));
    }

    // API routes
    if domain.routes.is_empty() {
        sections.push("## API routes\n_None detected._".to_string());
    } else {
        let routes_list: String = domain
            .routes
            .iter()
            .map(|r| format!("- {} `[seen-in-code]`", r))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## API routes\n{}", routes_list));
    }

    // Dependencies
    if domain.dependencies.is_empty() {
        sections.push("## Dependencies\n_None detected._".to_string());
    } else {
        let deps_list: String = domain
            .dependencies
            .iter()
            .map(|d| {
                format!(
                    "- [{}](../{d}/_overview.md) — imports from {} module",
                    capitalize(d),
                    d,
                    d = d
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Dependencies\n{}", deps_list));
    }

    // Referenced by
    let referenced_by: Vec<&DomainInfo> = all_domains
        .iter()
        .filter(|d| d.name != domain.name && d.dependencies.contains(&domain.name))
        .collect();

    if referenced_by.is_empty() {
        sections.push("## Referenced by\n_None detected._".to_string());
    } else {
        let refs_list: String = referenced_by
            .iter()
            .map(|d| {
                format!(
                    "- [{}](../{d}/_overview.md)",
                    capitalize(&d.name),
                    d = d.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Referenced by\n{}", refs_list));
    }

    // Test coverage
    if domain.test_files.is_empty() {
        sections.push("## Test coverage\n_No test files detected._".to_string());
    } else {
        let test_list: String = domain
            .test_files
            .iter()
            .map(|t| format!("- {}", t))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Test coverage\n{}", test_list));
    }

    sections.join("\n\n")
}

pub fn generate_graph(domains: &[DomainInfo]) -> String {
    let mut mermaid_lines = Vec::new();

    for domain in domains {
        for dep in &domain.dependencies {
            mermaid_lines.push(format!("    {} --> {}", domain.name, dep));
        }
    }

    // Also add isolated domains (no deps, no reverse deps)
    let connected: HashSet<&str> = domains
        .iter()
        .flat_map(|d| {
            let mut names = vec![d.name.as_str()];
            names.extend(d.dependencies.iter().map(|s| s.as_str()));
            names
        })
        .filter(|name| {
            domains.iter().any(|d| {
                d.name == *name
                    && (!d.dependencies.is_empty()
                        || domains.iter().any(|other| other.dependencies.contains(&d.name)))
            })
        })
        .collect();

    for domain in domains {
        if !connected.contains(domain.name.as_str()) && domain.dependencies.is_empty() {
            mermaid_lines.push(format!("    {}", domain.name));
        }
    }

    let graph_body = if mermaid_lines.is_empty() {
        "    %% No dependencies detected".to_string()
    } else {
        mermaid_lines.join("\n")
    };

    format!(
        "# Domain dependency graph\n\n\
         > Auto-generated from codebase scan. Do not edit manually.\n\n\
         ```mermaid\n\
         graph LR\n\
         {}\n\
         ```\n",
        graph_body
    )
}

pub fn generate_index(domains: &[DomainInfo], date: &str) -> String {
    let domain_list = if domains.is_empty() {
        "_No domains documented yet._".to_string()
    } else {
        domains
            .iter()
            .map(|d| {
                format!(
                    "- [{}](domains/{d}/_overview.md) — {} files, {} models `[inferred]`",
                    capitalize(&d.name),
                    d.files.len(),
                    d.models.len(),
                    d = d.name,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Project Wiki\n\n\
         > Auto-generated knowledge base. Managed by [project-wiki](https://github.com/agencedebord/project-wiki).\n\n\
         ## Domains\n\n\
         {}\n\n\
         ## Recent decisions\n\n\
         | Date | Decision | Domain |\n\
         |------|----------|--------|\n\n\
         ## Last updated\n\n\
         - Initialized on {}\n",
        domain_list, date
    )
}

pub fn generate_needs_review(domains: &[DomainInfo]) -> String {
    let mut questions: Vec<String> = Vec::new();

    for domain in domains {
        for comment in &domain.comments {
            questions.push(format!("- **{}**: {}", capitalize(&domain.name), comment));
        }
    }

    let questions_section = if questions.is_empty() {
        "_No open questions found._".to_string()
    } else {
        questions.join("\n")
    };

    format!(
        "# Needs review\n\n\
         > Items below were generated automatically and need human validation.\n\
         > Answer or validate each item, then remove it from this list.\n\n\
         ## Open questions\n\n\
         {}\n\n\
         ## Unresolved contradictions\n\n\
         _None detected._\n",
        questions_section
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(strip_all_extensions("billing.controller.ts"), "billing.controller");
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
        // strip_all_extensions recursively strips short extensions and known source extensions
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

    // ─── generate_domain_overview ───

    #[test]
    fn generate_overview_contains_domain_name() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/invoice.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };

        let overview = generate_domain_overview(&domain, &[domain.clone()]);
        assert!(overview.contains("Billing"));
        assert!(overview.contains("Invoice"));
        assert!(overview.contains("inferred"));
    }

    // ─── generate_graph ───

    #[test]
    fn generate_graph_with_no_deps() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let graph = generate_graph(&domains);
        assert!(graph.contains("billing"));
        assert!(graph.contains("mermaid"));
    }

    #[test]
    fn generate_graph_with_deps() {
        let domains = vec![
            DomainInfo {
                name: "billing".to_string(),
                files: vec![],
                dependencies: vec!["users".to_string()],
                models: vec![],
                routes: vec![],
                comments: vec![],
                test_files: vec![],
            },
            DomainInfo {
                name: "users".to_string(),
                files: vec![],
                dependencies: vec![],
                models: vec![],
                routes: vec![],
                comments: vec![],
                test_files: vec![],
            },
        ];

        let graph = generate_graph(&domains);
        assert!(graph.contains("billing --> users"));
    }

    // ─── generate_index ───

    #[test]
    fn generate_index_contains_domain_entries() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec!["a.ts".to_string(), "b.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let index = generate_index(&domains, "2025-01-01");
        assert!(index.contains("Billing"));
        assert!(index.contains("2 files"));
        assert!(index.contains("1 models"));
    }

    // ─── generate_needs_review ───

    #[test]
    fn generate_needs_review_with_comments() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec!["[TODO] Fix invoice calculation".to_string()],
            test_files: vec![],
        }];

        let review = generate_needs_review(&domains);
        assert!(review.contains("Fix invoice calculation"));
        assert!(review.contains("Billing"));
    }

    #[test]
    fn generate_needs_review_empty() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let review = generate_needs_review(&domains);
        assert!(review.contains("No open questions found"));
    }
}
