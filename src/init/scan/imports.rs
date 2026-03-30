use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rayon::prelude::*;
use regex::Regex;

// ─── Pre-compiled regex patterns ───

// JS/TS imports
static RE_JS_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap());
static RE_JS_REQUIRE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"require\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap());
static RE_JS_EXPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"export\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap());
// Dynamic import: import('...')
static RE_JS_DYNAMIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap());

// Python imports
static RE_PY_FROM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^from\s+(\S+)\s+import").unwrap());
static RE_PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^import\s+(\S+)").unwrap());

// Rust imports
static RE_RS_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"use\s+crate::(\S+?)(?:::\{|;)").unwrap());
static RE_RS_MOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:pub\s+)?mod\s+(\w+)\s*;").unwrap());

// Go imports
static RE_GO_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\s+"([^"]+)""#).unwrap());
static RE_GO_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\s*\(([\s\S]*?)\)"#).unwrap());
static RE_GO_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)""#).unwrap());

// ─── Public types ───

#[derive(Debug, Default)]
pub struct FileImports {
    pub file_path: PathBuf,
    pub imports: Vec<String>,
}

// ─── Import extraction ───

pub fn extract_all_imports(files: &[&PathBuf], _root: &Path) -> Vec<FileImports> {
    files
        .par_iter()
        .filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
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
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

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

    // Dynamic import: import('...')
    for cap in RE_JS_DYNAMIC.captures_iter(content) {
        imports.push(cap[1].to_string());
    }

    // Resolve @/ path alias to src/ (common TS convention)
    imports = imports
        .into_iter()
        .map(|imp| resolve_ts_path_alias(&imp))
        .collect();

    // Filter out external packages (node_modules) — keep only relative & aliased imports
    imports.retain(|imp| is_local_import(imp));

    imports
}

/// Resolve common TypeScript path aliases.
/// `@/billing/invoice` → `src/billing/invoice`
/// `~/billing/invoice` → `src/billing/invoice`
fn resolve_ts_path_alias(import: &str) -> String {
    if let Some(rest) = import.strip_prefix("@/") {
        return format!("src/{rest}");
    }
    if let Some(rest) = import.strip_prefix("~/") {
        return format!("src/{rest}");
    }
    import.to_string()
}

/// Returns true if the import looks like a local/project import (not an npm package).
/// Local imports start with `.`, `..`, `src/`, or look like scoped project packages.
fn is_local_import(import: &str) -> bool {
    // Relative imports are always local
    if import.starts_with('.') {
        return true;
    }
    // Already resolved path alias
    if import.starts_with("src/") {
        return true;
    }
    // Scoped packages like @myorg/package — these are usually npm packages, skip them.
    // But @/ and ~/ are already resolved above, so anything starting with @ here is npm.
    if import.starts_with('@') {
        return false;
    }
    // Bare specifiers (no path prefix) are npm packages: 'express', 'react', 'lodash'
    // Exception: single-word imports that contain a slash might be local (rare)
    if !import.contains('/') {
        return false;
    }
    // Things like 'next/router', 'react-dom/client' are npm packages
    // Heuristic: if no dot/relative prefix and it looks like a package name, skip it.
    // Known npm package prefixes to exclude:
    false
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

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ts_type_only_import() {
        let content = r#"import type { Invoice } from './billing/invoice';"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["./billing/invoice"]);
    }

    #[test]
    fn test_ts_reexport() {
        let content = r#"export * from './billing/types';"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["./billing/types"]);
    }

    #[test]
    fn test_ts_named_reexport() {
        let content = r#"export { Invoice, LineItem } from '../billing/models';"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["../billing/models"]);
    }

    #[test]
    fn test_ts_dynamic_import() {
        let content = r#"const mod = await import('./billing/heavy');"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["./billing/heavy"]);
    }

    #[test]
    fn test_ts_path_alias_at() {
        let content = r#"import { Invoice } from '@/billing/invoice';"#;
        let imports = extract_js_imports(content);
        // @/ resolved to src/
        assert_eq!(imports, vec!["src/billing/invoice"]);
    }

    #[test]
    fn test_ts_path_alias_tilde() {
        let content = r#"import { Invoice } from '~/billing/invoice';"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["src/billing/invoice"]);
    }

    #[test]
    fn test_ts_external_import_filtered() {
        let content = r#"
import express from 'express';
import { useState } from 'react';
import { Invoice } from './billing/invoice';
"#;
        let imports = extract_js_imports(content);
        // Only local import should remain
        assert_eq!(imports, vec!["./billing/invoice"]);
    }

    #[test]
    fn test_ts_scoped_npm_package_filtered() {
        let content = r#"
import { Client } from '@prisma/client';
import { z } from 'zod';
import { Invoice } from '../billing/invoice';
"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["../billing/invoice"]);
    }

    #[test]
    fn test_ts_npm_with_subpath_filtered() {
        let content = r#"
import { useRouter } from 'next/router';
import { createClient } from 'redis/client';
import { handler } from './auth/handler';
"#;
        let imports = extract_js_imports(content);
        assert_eq!(imports, vec!["./auth/handler"]);
    }

    #[test]
    fn test_resolve_ts_path_alias() {
        assert_eq!(
            resolve_ts_path_alias("@/billing/invoice"),
            "src/billing/invoice"
        );
        assert_eq!(
            resolve_ts_path_alias("~/billing/invoice"),
            "src/billing/invoice"
        );
        assert_eq!(
            resolve_ts_path_alias("./billing/invoice"),
            "./billing/invoice"
        );
        assert_eq!(resolve_ts_path_alias("express"), "express");
    }

    #[test]
    fn test_is_local_import() {
        // Local
        assert!(is_local_import("./billing/invoice"));
        assert!(is_local_import("../auth/handler"));
        assert!(is_local_import("src/billing/invoice"));

        // External
        assert!(!is_local_import("express"));
        assert!(!is_local_import("react"));
        assert!(!is_local_import("@prisma/client"));
        assert!(!is_local_import("next/router"));
    }
}
