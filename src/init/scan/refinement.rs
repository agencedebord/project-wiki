use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::structure::{is_source_file, normalize_domain_name, strip_all_extensions};
use super::DomainFileMap;
use crate::ui;

// ─── Constants ───

/// Domains with more source files than this threshold are candidates for splitting.
const REFINEMENT_THRESHOLD: usize = 25;

/// Minimum source files in a subdirectory to form its own sub-domain.
const MIN_CLUSTER_SIZE: usize = 3;

/// Minimum prefix length (in characters) to be considered meaningful.
const MIN_PREFIX_LENGTH: usize = 3;

/// Minimum files sharing a prefix to form a cluster.
const MIN_PREFIX_CLUSTER_SIZE: usize = 3;

/// Maximum iterations of the split loop to prevent infinite recursion.
/// In practice, 3 rounds is enough: e.g., frontend → frontend-src → frontend-src-components → done.
const MAX_REFINEMENT_ROUNDS: usize = 5;

/// Segments in domain names that are structural noise and should be stripped
/// when simplifying names after refinement.
/// These are directory names that organize code by type (not by domain).
const NOISE_SEGMENTS: &[&str] = &[
    "src",
    "components",
    "lib",
    "app",
    "pages",
    "modules",
    "features",
    "packages",
    "core",
    "internal",
    "public",
    "private",
    "(private)",
    "(public)",
];

// ─── Public entry point ───

/// Refine oversized domains by splitting them into meaningful sub-domains.
///
/// This is a transparent transformation: input and output are both `DomainFileMap`.
/// Domains with fewer than `REFINEMENT_THRESHOLD` source files pass through unchanged.
///
/// The refinement applies two strategies in order:
/// 1. **Subdirectory splitting**: files in distinct subdirectories become sub-domains
/// 2. **Prefix clustering**: remaining flat files are grouped by common name prefix
///
/// After splitting, a cross-directory merge pass combines clusters with the same
/// semantic name across formerly separate domains (e.g., "document" files from
/// both "components" and "hooks" merge into a single "document" domain).
pub(crate) fn refine_domains(domains: DomainFileMap) -> DomainFileMap {
    // Maps split product domain name → its cluster suffix (the semantic part after the parent prefix).
    // We track this explicitly because the suffix itself can contain hyphens
    // (e.g., "components-real-estate-file" has suffix "real-estate-file", not "file").
    let mut split_suffixes: HashMap<String, String> = HashMap::new();

    // Phase 1: Iteratively split oversized domains until convergence.
    // A single pass may produce sub-domains that are themselves oversized
    // (e.g., frontend → frontend-src still has 170+ files), so we iterate.
    let mut refined = domains;
    for _round in 0..MAX_REFINEMENT_ROUNDS {
        let mut next = DomainFileMap::new();
        let mut any_split = false;

        for (name, files) in refined {
            let source_count = count_source_files(&files);
            if source_count <= REFINEMENT_THRESHOLD {
                next.insert(name, files);
                continue;
            }

            let (split, suffixes) = split_domain(&name, files);
            if split.len() > 1 {
                any_split = true;
                let sub_names: Vec<&String> = split.keys().collect();
                ui::verbose(&format!(
                    "refined {:?} ({} files) into {} sub-domains: {:?}",
                    name, source_count, split.len(), sub_names
                ));
            }
            split_suffixes.extend(suffixes);
            next.extend(split);
        }

        refined = next;
        if !any_split {
            break;
        }
    }

    // Phase 2: Cross-directory merge
    if !split_suffixes.is_empty() {
        merge_cross_domain_clusters(&mut refined, &split_suffixes);
    }

    // Phase 3: Simplify domain names by removing structural noise segments
    // e.g., "frontend-src-components-forms-step" → "frontend-forms-step"
    simplify_domain_names(&mut refined);

    refined
}

// ─── Domain splitting ───

/// Split a single oversized domain into sub-domains.
///
/// Strategy:
/// 1. Group files by their immediate subdirectory (relative to the domain's common root).
/// 2. Subdirectories with >= MIN_CLUSTER_SIZE source files become their own sub-domain.
/// 3. Remaining files go to prefix clustering.
/// 4. Files that still don't cluster go to "{name}-common".
///
/// If splitting produces only one bucket, reverts to the original domain name.
///
/// Returns (domain_map, suffixes) where suffixes maps each new domain name
/// to its cluster suffix (needed for correct cross-directory merge).
fn split_domain(name: &str, files: Vec<PathBuf>) -> (DomainFileMap, HashMap<String, String>) {
    let common_root = find_common_root(&files);
    let (subdir_groups, root_files) = group_by_subdirectory(files, &common_root);

    let mut result = DomainFileMap::new();
    let mut suffixes: HashMap<String, String> = HashMap::new();
    let mut remainder: Vec<PathBuf> = root_files;

    // Subdirectory-based sub-domains
    for (subdir_name, subdir_files) in subdir_groups {
        if count_source_files(&subdir_files) >= MIN_CLUSTER_SIZE {
            let suffix = normalize_domain_name(&subdir_name);
            let sub_domain_name = format!("{}-{}", name, suffix);
            suffixes.insert(sub_domain_name.clone(), suffix);
            result.insert(sub_domain_name, subdir_files);
        } else {
            remainder.extend(subdir_files);
        }
    }

    // Prefix clustering on the remainder
    if !remainder.is_empty() {
        let (clusters, unclustered) = cluster_by_prefix(&remainder);
        for (prefix, cluster_files) in clusters {
            let sub_domain_name = format!("{}-{}", name, prefix);
            suffixes.insert(sub_domain_name.clone(), prefix);
            result.insert(sub_domain_name, cluster_files);
        }
        if !unclustered.is_empty() {
            if result.is_empty() {
                // No clusters found at all — keep original name
                result.insert(name.to_string(), unclustered);
            } else {
                result.insert(format!("{}-common", name), unclustered);
            }
        }
    }

    // If splitting produced only one sub-domain, revert to original name
    if result.len() <= 1 {
        let mut all_files: Vec<PathBuf> = Vec::new();
        for (_, f) in result.drain() {
            all_files.extend(f);
        }
        let mut reverted = DomainFileMap::new();
        reverted.insert(name.to_string(), all_files);
        return (reverted, HashMap::new());
    }

    (result, suffixes)
}

// ─── Path helpers ───

/// Find the longest common directory prefix of a set of file paths.
fn find_common_root(files: &[PathBuf]) -> PathBuf {
    if files.is_empty() {
        return PathBuf::new();
    }
    let first_parent = files[0].parent().unwrap_or(Path::new(""));
    let mut common = first_parent.to_path_buf();
    for file in &files[1..] {
        let parent = file.parent().unwrap_or(Path::new(""));
        common = common_ancestor(&common, parent);
    }
    common
}

fn common_ancestor(a: &Path, b: &Path) -> PathBuf {
    let a_components: Vec<_> = a.components().collect();
    let b_components: Vec<_> = b.components().collect();
    let mut result = PathBuf::new();
    for (ac, bc) in a_components.iter().zip(b_components.iter()) {
        if ac == bc {
            result.push(ac);
        } else {
            break;
        }
    }
    result
}

/// Group files by their immediate subdirectory relative to a common root.
///
/// Consumes the input Vec to avoid unnecessary cloning.
/// Returns (subdirectory_groups, root_files) where root_files are files
/// directly in the common root (no intermediate subdirectory).
fn group_by_subdirectory(
    files: Vec<PathBuf>,
    common_root: &Path,
) -> (HashMap<String, Vec<PathBuf>>, Vec<PathBuf>) {
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut root_files: Vec<PathBuf> = Vec::new();

    for file in files {
        let is_root = match file.strip_prefix(common_root) {
            Ok(rel) => {
                let components: Vec<_> = rel.components().collect();
                if components.len() > 1 {
                    let subdir = components[0].as_os_str().to_string_lossy().to_string();
                    groups.entry(subdir).or_default().push(file);
                    continue;
                }
                true
            }
            Err(_) => true,
        };
        if is_root {
            root_files.push(file);
        }
    }

    (groups, root_files)
}

// ─── Prefix clustering ───

/// Cluster files by common naming prefix.
///
/// Only groups with >= MIN_PREFIX_CLUSTER_SIZE files are returned as clusters.
/// All other files go into the unclustered bucket.
fn cluster_by_prefix(files: &[PathBuf]) -> (HashMap<String, Vec<PathBuf>>, Vec<PathBuf>) {
    let mut prefix_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut no_prefix: Vec<PathBuf> = Vec::new();

    for file in files {
        if let Some(prefix) = extract_semantic_prefix(file) {
            prefix_groups.entry(prefix).or_default().push(file.clone());
        } else {
            no_prefix.push(file.clone());
        }
    }

    let mut clusters: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut unclustered: Vec<PathBuf> = no_prefix;

    for (prefix, group_files) in prefix_groups {
        if group_files.len() >= MIN_PREFIX_CLUSTER_SIZE {
            clusters.insert(prefix, group_files);
        } else {
            unclustered.extend(group_files);
        }
    }

    (clusters, unclustered)
}

/// Extract a semantic prefix from a file's name.
///
/// Examples:
///   "useDocumentUpload.ts" -> "document"
///   "DocumentList.tsx"     -> "document"
///   "document-editor.ts"  -> "document"
///   "document_utils.py"   -> "document"
///   "index.ts"            -> None (non-semantic)
///   "App.tsx"             -> None (too short)
fn extract_semantic_prefix(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let stem = strip_all_extensions(stem);

    // Skip non-semantic file names
    let lower = stem.to_lowercase();
    if matches!(
        lower.as_str(),
        "index"
            | "mod"
            | "main"
            | "app"
            | "root"
            | "layout"
            | "page"
            | "route"
            | "utils"
            | "helpers"
            | "types"
            | "constants"
            | "config"
            | "setup"
            | "lib"
            | "init"
            | "common"
            | "shared"
            | "base"
            | "global"
    ) {
        return None;
    }

    // Strip common functional prefixes (React hooks, etc.)
    let cleaned = strip_functional_prefix(&stem);

    // Split into words on camelCase / PascalCase / hyphen / underscore boundaries
    let words = split_into_words(&cleaned);
    let first_word = words.first()?;

    if first_word.len() < MIN_PREFIX_LENGTH {
        return None;
    }

    Some(first_word.to_lowercase())
}

/// Strip known functional prefixes like "use" (React hooks).
///
/// "useDocumentUpload" -> "DocumentUpload"
/// "user" -> "user" (not stripped: next char is lowercase)
fn strip_functional_prefix(name: &str) -> String {
    if name.len() > 4 && name.starts_with("use") {
        let after = &name[3..];
        // Only strip if what follows starts with uppercase (camelCase boundary)
        if after.starts_with(|c: char| c.is_uppercase()) {
            return after.to_string();
        }
    }
    name.to_string()
}

/// Split a name into words on camelCase, PascalCase, hyphen, or underscore boundaries.
///
/// "DocumentUploadForm" -> ["Document", "Upload", "Form"]
/// "document-editor"    -> ["document", "editor"]
/// "document_utils"     -> ["document", "utils"]
fn split_into_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else if ch.is_uppercase() && !current.is_empty() {
            let prev_is_lower = current.chars().last().is_some_and(|c| c.is_lowercase());
            if prev_is_lower {
                words.push(current.clone());
                current.clear();
            }
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }

    words
}

// ─── Cross-directory merge ───

/// Merge sub-domains that share the same cluster suffix across different parent domains.
///
/// After splitting, domain names follow the pattern "{parent}-{cluster}".
/// If multiple parents produced a cluster with the same name, merge them.
///
/// "components-document" + "hooks-document" -> "document"
///
/// Uses `split_suffixes` (domain_name → cluster_suffix) to correctly extract
/// the suffix without parsing hyphens in the domain name. This avoids bugs
/// with compound suffixes like "real-estate-file" being truncated to "file".
fn merge_cross_domain_clusters(
    domains: &mut DomainFileMap,
    split_suffixes: &HashMap<String, String>,
) {
    // Build suffix -> list of full domain names
    let mut suffix_groups: HashMap<String, Vec<String>> = HashMap::new();
    let split_product_names: HashSet<&String> = split_suffixes.keys().collect();

    for (domain_name, suffix) in split_suffixes {
        if suffix == "common" {
            continue;
        }
        suffix_groups
            .entry(suffix.clone())
            .or_default()
            .push(domain_name.clone());
    }

    let mut merges: Vec<(Vec<String>, String)> = Vec::new();

    for (suffix, source_names) in &suffix_groups {
        if source_names.len() < 2 {
            continue;
        }

        // Check that the target name won't conflict with a non-split-product domain
        let target_exists_as_original =
            domains.contains_key(suffix) && !split_product_names.contains(suffix);
        if target_exists_as_original {
            // A pre-existing domain with this name exists; skip merge to avoid confusion
            continue;
        }

        merges.push((source_names.clone(), suffix.clone()));
    }

    for (sources, target) in merges {
        let mut merged_files: Vec<PathBuf> = Vec::new();
        for source in &sources {
            if let Some(files) = domains.remove(source) {
                merged_files.extend(files);
            }
        }
        if !merged_files.is_empty() {
            domains.entry(target).or_default().extend(merged_files);
        }
    }
}

// ─── Name simplification ───

/// Simplify domain names by removing structural noise segments.
///
/// "frontend-src-components-forms-step" → "frontend-forms-step"
/// "frontend-src-api" → "frontend-api"
/// "frontend-src-components-dashboard" → "frontend-dashboard"
///
/// Handles collisions: if simplification would create a duplicate name,
/// progressively keeps more segments until the name is unique.
fn simplify_domain_names(domains: &mut DomainFileMap) {
    let original_names: Vec<String> = domains.keys().cloned().collect();
    let mut renames: Vec<(String, String)> = Vec::new();

    for name in &original_names {
        let simplified = simplify_name(name);
        if simplified != *name {
            renames.push((name.clone(), simplified));
        }
    }

    // Resolve collisions: if two names simplify to the same thing,
    // keep their original names instead
    let mut target_counts: HashMap<String, usize> = HashMap::new();
    for (_, target) in &renames {
        *target_counts.entry(target.clone()).or_default() += 1;
    }
    // Also check collision with names that weren't renamed
    let renamed_originals: HashSet<&String> = renames.iter().map(|(orig, _)| orig).collect();
    for name in &original_names {
        if !renamed_originals.contains(name) {
            *target_counts.entry(name.clone()).or_default() += 1;
        }
    }

    for (original, target) in renames {
        if target_counts[&target] > 1 {
            // Collision — keep original name
            continue;
        }
        if let Some(files) = domains.remove(&original) {
            domains.insert(target, files);
        }
    }
}

/// Remove noise segments from a domain name.
///
/// Splits on hyphens, drops segments that are in NOISE_SEGMENTS,
/// and rejoins. Preserves at least the first and last segment
/// to maintain meaning.
fn simplify_name(name: &str) -> String {
    let segments: Vec<&str> = name.split('-').collect();
    if segments.len() <= 2 {
        return name.to_string();
    }

    // Keep first segment (top-level context, e.g., "frontend")
    // and filter noise from the middle, always keep the last segment
    let first = segments[0];
    let last = segments[segments.len() - 1];
    let middle: Vec<&str> = segments[1..segments.len() - 1]
        .iter()
        .filter(|s| !NOISE_SEGMENTS.contains(s))
        .copied()
        .collect();

    let mut result = vec![first];
    result.extend(middle);
    // Avoid duplicating the last segment if it's the same as what we already have
    if result.last().copied() != Some(last) {
        result.push(last);
    }

    let simplified = result.join("-");

    // If we stripped everything except first, return "first-last"
    if simplified == first && first != last {
        return format!("{}-{}", first, last);
    }

    simplified
}

// ─── Helpers ───

fn count_source_files(files: &[PathBuf]) -> usize {
    files.iter().filter(|f| is_source_file(f)).count()
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    // ─── split_into_words ───

    #[test]
    fn split_camel_case() {
        assert_eq!(
            split_into_words("DocumentUploadForm"),
            vec!["Document", "Upload", "Form"]
        );
    }

    #[test]
    fn split_kebab_case() {
        assert_eq!(
            split_into_words("document-editor"),
            vec!["document", "editor"]
        );
    }

    #[test]
    fn split_snake_case() {
        assert_eq!(
            split_into_words("document_utils"),
            vec!["document", "utils"]
        );
    }

    #[test]
    fn split_single_word() {
        assert_eq!(split_into_words("billing"), vec!["billing"]);
    }

    #[test]
    fn split_all_caps_sequence() {
        // "XMLParser" — X, M, L are all uppercase; should not split each letter
        assert_eq!(split_into_words("XMLParser"), vec!["XMLParser"]);
    }

    // ─── strip_functional_prefix ───

    #[test]
    fn strips_use_prefix_for_hooks() {
        assert_eq!(
            strip_functional_prefix("useDocumentUpload"),
            "DocumentUpload"
        );
    }

    #[test]
    fn does_not_strip_user() {
        assert_eq!(strip_functional_prefix("user"), "user");
    }

    #[test]
    fn does_not_strip_useful() {
        assert_eq!(strip_functional_prefix("useful"), "useful");
    }

    #[test]
    fn does_not_strip_use_alone() {
        // "use" is only 3 chars, below the len > 4 guard
        assert_eq!(strip_functional_prefix("use"), "use");
    }

    #[test]
    fn strips_use_with_pascal() {
        assert_eq!(strip_functional_prefix("useAuth"), "Auth");
    }

    // ─── extract_semantic_prefix ───

    #[test]
    fn prefix_from_hook() {
        let p = PathBuf::from("src/hooks/useDocumentUpload.ts");
        assert_eq!(extract_semantic_prefix(&p), Some("document".into()));
    }

    #[test]
    fn prefix_from_component() {
        let p = PathBuf::from("src/components/DocumentList.tsx");
        assert_eq!(extract_semantic_prefix(&p), Some("document".into()));
    }

    #[test]
    fn prefix_from_kebab() {
        let p = PathBuf::from("src/components/document-editor.ts");
        assert_eq!(extract_semantic_prefix(&p), Some("document".into()));
    }

    #[test]
    fn prefix_from_snake() {
        let p = PathBuf::from("src/hooks/document_upload.py");
        assert_eq!(extract_semantic_prefix(&p), Some("document".into()));
    }

    #[test]
    fn no_prefix_from_index() {
        let p = PathBuf::from("src/components/index.ts");
        assert_eq!(extract_semantic_prefix(&p), None);
    }

    #[test]
    fn no_prefix_from_utils() {
        let p = PathBuf::from("src/hooks/utils.ts");
        assert_eq!(extract_semantic_prefix(&p), None);
    }

    #[test]
    fn no_prefix_from_short_name() {
        let p = PathBuf::from("src/components/UI.tsx");
        assert_eq!(extract_semantic_prefix(&p), None);
    }

    // ─── find_common_root ───

    #[test]
    fn common_root_same_dir() {
        let files = vec![
            PathBuf::from("src/components/A.tsx"),
            PathBuf::from("src/components/B.tsx"),
        ];
        assert_eq!(find_common_root(&files), PathBuf::from("src/components"));
    }

    #[test]
    fn common_root_different_subdirs() {
        let files = vec![
            PathBuf::from("src/components/forms/A.tsx"),
            PathBuf::from("src/components/tables/B.tsx"),
        ];
        assert_eq!(find_common_root(&files), PathBuf::from("src/components"));
    }

    #[test]
    fn common_root_deeply_nested() {
        let files = vec![
            PathBuf::from("a/b/c/d/file1.ts"),
            PathBuf::from("a/b/x/y/file2.ts"),
        ];
        assert_eq!(find_common_root(&files), PathBuf::from("a/b"));
    }

    // ─── refine_domains (integration) ───

    #[test]
    fn small_domain_unchanged() {
        let mut domains = DomainFileMap::new();
        domains.insert(
            "billing".into(),
            vec![
                PathBuf::from("src/services/billing/invoice.ts"),
                PathBuf::from("src/services/billing/payment.ts"),
                PathBuf::from("src/services/billing/subscription.ts"),
            ],
        );
        let original = domains.clone();
        let refined = refine_domains(domains);
        assert_eq!(refined, original);
    }

    #[test]
    fn oversized_domain_splits_by_subdirectory() {
        let mut domains = DomainFileMap::new();
        let mut files = Vec::new();
        // 10 files in each of 3 subdirectories = 30 files > threshold
        for subdir in &["forms", "tables", "modals"] {
            for i in 0..10 {
                files.push(PathBuf::from(format!(
                    "src/components/{}/Widget{}.tsx",
                    subdir, i
                )));
            }
        }
        domains.insert("components".into(), files);
        let refined = refine_domains(domains);

        assert!(
            refined.contains_key("components-forms"),
            "Expected 'components-forms', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            refined.contains_key("components-tables"),
            "Expected 'components-tables', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            refined.contains_key("components-modals"),
            "Expected 'components-modals', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            !refined.contains_key("components"),
            "Original domain should be gone"
        );
    }

    #[test]
    fn flat_files_clustered_by_prefix() {
        let mut domains = DomainFileMap::new();
        let mut files = Vec::new();
        // 5 Document* files + 5 User* files + 5 Invoice* files + misc
        for i in 0..5 {
            files.push(PathBuf::from(format!(
                "src/components/Document{}.tsx",
                ["List", "Editor", "Preview", "Upload", "Status"][i]
            )));
            files.push(PathBuf::from(format!(
                "src/components/User{}.tsx",
                ["Avatar", "Profile", "Settings", "Card", "Menu"][i]
            )));
            files.push(PathBuf::from(format!(
                "src/components/Invoice{}.tsx",
                ["Table", "Form", "Preview", "Summary", "Detail"][i]
            )));
        }
        // Add some misc files that won't cluster
        for i in 0..12 {
            files.push(PathBuf::from(format!(
                "src/components/Misc{}.tsx",
                ('A' as u8 + i as u8) as char
            )));
        }
        domains.insert("components".into(), files);
        let refined = refine_domains(domains);

        assert!(
            refined.contains_key("components-document"),
            "Expected 'components-document', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            refined.contains_key("components-user"),
            "Expected 'components-user', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            refined.contains_key("components-invoice"),
            "Expected 'components-invoice', found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn single_subdir_domain_not_split() {
        let mut domains = DomainFileMap::new();
        let files: Vec<PathBuf> = (0..30)
            .map(|i| PathBuf::from(format!("src/components/forms/Field{}.tsx", i)))
            .collect();
        domains.insert("components".into(), files);
        let refined = refine_domains(domains);

        // All files in one subdir -> should revert to original name
        assert!(
            refined.contains_key("components"),
            "Expected original 'components' name preserved, found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert_eq!(refined.len(), 1);
    }

    #[test]
    fn cross_domain_merge_combines_same_cluster() {
        // Simulate two domains that both split and produce a "document" cluster
        let mut domains = DomainFileMap::new();

        // "components" domain with 30+ files across subdirs
        let mut comp_files = Vec::new();
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/document/Doc{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/tables/Table{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/charts/Chart{}.tsx",
                i
            )));
        }
        domains.insert("components".into(), comp_files);

        // "hooks" domain with 30+ files across subdirs
        let mut hook_files = Vec::new();
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/document/useDoc{}.ts",
                i
            )));
        }
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/auth/useAuth{}.ts",
                i
            )));
        }
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/billing/useBilling{}.ts",
                i
            )));
        }
        domains.insert("hooks".into(), hook_files);

        let refined = refine_domains(domains);

        // "components-document" and "hooks-document" should merge into "document"
        assert!(
            refined.contains_key("document"),
            "Expected merged 'document' domain, found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            !refined.contains_key("components-document"),
            "components-document should be merged"
        );
        assert!(
            !refined.contains_key("hooks-document"),
            "hooks-document should be merged"
        );
        // Other clusters should remain qualified (only one source each)
        assert!(refined.contains_key("components-tables"));
        assert!(refined.contains_key("components-charts"));
        assert!(refined.contains_key("hooks-auth"));
        assert!(refined.contains_key("hooks-billing"));
    }

    #[test]
    fn merge_does_not_clobber_existing_domain() {
        let mut domains = DomainFileMap::new();

        // Pre-existing "auth" domain (small, not split)
        domains.insert(
            "auth".into(),
            vec![
                PathBuf::from("src/services/auth/login.ts"),
                PathBuf::from("src/services/auth/register.ts"),
            ],
        );

        // Large "components" domain that will split and produce a "components-auth" cluster
        let mut comp_files = Vec::new();
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/auth/Auth{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/billing/Bill{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/dashboard/Dash{}.tsx",
                i
            )));
        }
        domains.insert("components".into(), comp_files);

        let refined = refine_domains(domains);

        // "auth" pre-existed and was not a split product, so merge should not clobber it
        assert!(
            refined.contains_key("auth"),
            "Pre-existing 'auth' domain should survive"
        );
        // components-auth should remain as-is (no merge partner that's also a split product)
        assert!(
            refined.contains_key("components-auth"),
            "components-auth should stay qualified (can't merge into pre-existing 'auth')"
        );
    }

    #[test]
    fn cross_domain_merge_with_compound_suffix() {
        // Regression test: subdirectories with hyphens in their name
        // must merge correctly (not truncate to the last hyphen segment)
        let mut domains = DomainFileMap::new();

        // "components" domain with a "real-estate-file" subdir
        let mut comp_files = Vec::new();
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/real-estate-file/Ref{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/dashboard/Dash{}.tsx",
                i
            )));
        }
        for i in 0..10 {
            comp_files.push(PathBuf::from(format!(
                "src/components/settings/Set{}.tsx",
                i
            )));
        }
        domains.insert("components".into(), comp_files);

        // "hooks" domain with a "real-estate-file" subdir
        let mut hook_files = Vec::new();
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/real-estate-file/useRef{}.ts",
                i
            )));
        }
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/auth/useAuth{}.ts",
                i
            )));
        }
        for i in 0..10 {
            hook_files.push(PathBuf::from(format!(
                "src/hooks/billing/useBill{}.ts",
                i
            )));
        }
        domains.insert("hooks".into(), hook_files);

        let refined = refine_domains(domains);

        // Should merge into "real-estate-file", NOT "file"
        assert!(
            refined.contains_key("real-estate-file"),
            "Expected merged 'real-estate-file' domain, found: {:?}",
            refined.keys().collect::<Vec<_>>()
        );
        assert!(
            !refined.contains_key("file"),
            "'file' should not exist — the full suffix is 'real-estate-file'"
        );
    }

    // ─── simplify_name ───

    #[test]
    fn simplify_strips_noise_segments() {
        assert_eq!(
            simplify_name("frontend-src-components-forms-step"),
            "frontend-forms-step"
        );
    }

    #[test]
    fn simplify_strips_src_and_components() {
        assert_eq!(
            simplify_name("frontend-src-components-dashboard"),
            "frontend-dashboard"
        );
    }

    #[test]
    fn simplify_preserves_short_names() {
        assert_eq!(simplify_name("frontend-api"), "frontend-api");
    }

    #[test]
    fn simplify_preserves_single_segment() {
        assert_eq!(simplify_name("billing"), "billing");
    }

    #[test]
    fn simplify_strips_src_only() {
        assert_eq!(simplify_name("frontend-src-api"), "frontend-api");
    }

    #[test]
    fn simplify_keeps_meaningful_middle() {
        assert_eq!(
            simplify_name("frontend-src-forms-realestatefile"),
            "frontend-forms-realestatefile"
        );
    }

    #[test]
    fn simplify_domain_names_handles_collision() {
        let mut domains = DomainFileMap::new();
        // Two domains that would both simplify to "frontend-dashboard"
        domains.insert(
            "frontend-src-components-dashboard".into(),
            vec![PathBuf::from("a.tsx")],
        );
        domains.insert(
            "frontend-src-app-dashboard".into(),
            vec![PathBuf::from("b.tsx")],
        );

        simplify_domain_names(&mut domains);

        // Both should keep their original names due to collision
        assert!(
            domains.contains_key("frontend-src-components-dashboard"),
            "Should keep original name due to collision, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
        assert!(
            domains.contains_key("frontend-src-app-dashboard"),
            "Should keep original name due to collision, found: {:?}",
            domains.keys().collect::<Vec<_>>()
        );
    }

    // ─── group_by_subdirectory ───

    #[test]
    fn groups_files_correctly() {
        let root = PathBuf::from("src/components");
        let files = vec![
            PathBuf::from("src/components/forms/A.tsx"),
            PathBuf::from("src/components/forms/B.tsx"),
            PathBuf::from("src/components/tables/C.tsx"),
            PathBuf::from("src/components/App.tsx"),
        ];
        let (groups, root_files) = group_by_subdirectory(files, &root);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups["forms"].len(), 2);
        assert_eq!(groups["tables"].len(), 1);
        assert_eq!(root_files.len(), 1);
    }
}
