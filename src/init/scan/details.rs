use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rayon::prelude::*;
use regex::Regex;

use super::structure::is_source_file;

// ─── Pre-compiled regex patterns ───

// Comments (TODO/FIXME/HACK/NOTE)
static RE_COMMENTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?://|#|/\*)\s*(TODO|FIXME|HACK|NOTE)\b[:\s]*(.*)").unwrap());

// Model/type definitions per language
static RE_JS_MODELS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:export\s+)?(?:interface|type|class|enum)\s+(\w+)").unwrap());
static RE_PY_CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"class\s+(\w+)").unwrap());
static RE_RS_STRUCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:pub\s+)?(?:struct|enum|trait)\s+(\w+)").unwrap());
static RE_GO_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"type\s+(\w+)\s+struct").unwrap());

// TypeScript: Zod schema definitions (e.g., const InvoiceSchema = z.object({...}))
static RE_ZOD_SCHEMA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:export\s+)?(?:const|let)\s+(\w+(?:Schema|Type|Validator))\s*=\s*z\.").unwrap()
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
static RE_ACTIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#\[\s*(get|post|put|patch|delete)\s*\(\s*"([^"]+)""#).unwrap());
static RE_GO_HTTP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:HandleFunc|Handle)\s*\(\s*"([^"]+)""#).unwrap());
// NestJS decorators: @Get('/path'), @Post('/path'), etc.
static RE_NESTJS_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@(Get|Post|Put|Patch|Delete)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
});
// NestJS controller prefix: @Controller('billing')
static RE_NESTJS_CONTROLLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"@Controller\s*\(\s*['"]([^'"]+)['"]"#).unwrap());

// ─── Types ───

#[derive(Debug, Clone)]
pub struct CodeComment {
    pub tag: String,
    pub text: String,
    pub file_path: String,
}

impl std::fmt::Display for CodeComment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.tag, self.text)
    }
}

#[derive(Debug, Default)]
pub struct DomainDetails {
    pub models: Vec<String>,
    pub routes: Vec<String>,
    pub comments: Vec<CodeComment>,
}

// ─── Detail extraction ───

pub fn extract_details(files: &[PathBuf]) -> DomainDetails {
    let results: Vec<DomainDetails> = files
        .par_iter()
        .filter_map(|path| {
            if !is_source_file(path) {
                return None;
            }
            let content = std::fs::read_to_string(path).ok()?;
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

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    // Extract TODO/FIXME/HACK/NOTE comments
    for cap in RE_COMMENTS.captures_iter(content) {
        let tag = &cap[1];
        let text = cap[2].trim().trim_end_matches("*/").trim();
        if !text.is_empty() {
            details.comments.push(CodeComment {
                tag: tag.to_string(),
                text: text.to_string(),
                file_path: path.to_string_lossy().to_string(),
            });
        }
    }

    // Extract model/type/struct/class/interface definitions
    extract_models(content, ext, &mut details);

    // Extract route/endpoint definitions
    extract_routes(content, ext, path, &mut details);

    details
}

fn extract_models(content: &str, ext: &str, details: &mut DomainDetails) {
    match ext {
        "ts" | "tsx" | "js" | "jsx" => {
            for cap in RE_JS_MODELS.captures_iter(content) {
                details.models.push(cap[1].to_string());
            }
            // Zod schemas: const InvoiceSchema = z.object({...})
            for cap in RE_ZOD_SCHEMA.captures_iter(content) {
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
}

fn extract_routes(content: &str, ext: &str, path: &Path, details: &mut DomainDetails) {
    // Express-style: app.get('/...'), router.post('/...')
    for cap in RE_EXPRESS.captures_iter(content) {
        details
            .routes
            .push(format!("{} {}", cap[1].to_uppercase(), &cap[2]));
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
                details.routes.push(format!("{} {}", &cap[1], route));
            }
        }
    }

    // NestJS decorators: @Get('/path'), @Post('/path')
    if ext == "ts" || ext == "tsx" {
        let controller_prefix = RE_NESTJS_CONTROLLER
            .captures(content)
            .map(|c| c[1].to_string());
        for cap in RE_NESTJS_ROUTE.captures_iter(content) {
            let method = cap[1].to_uppercase();
            let path = &cap[2];
            let full_path = match &controller_prefix {
                Some(prefix) => format!("{method} /{prefix}/{}", path.trim_start_matches('/')),
                None => format!("{method} {path}"),
            };
            details.routes.push(full_path);
        }
    }

    // Rust Actix/Axum style: #[get("/...")]
    for cap in RE_ACTIX.captures_iter(content) {
        details
            .routes
            .push(format!("{} {}", cap[1].to_uppercase(), &cap[2]));
    }

    // Go: http.HandleFunc("/...", handler)
    for cap in RE_GO_HTTP.captures_iter(content) {
        details.routes.push(cap[1].to_string());
    }
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zod_schema_detected() {
        let content = r#"
import { z } from 'zod';

export const InvoiceSchema = z.object({
    amount: z.number(),
    currency: z.string(),
});

const PaymentValidator = z.object({
    method: z.string(),
});
"#;
        let mut details = DomainDetails::default();
        extract_models(content, "ts", &mut details);
        assert!(details.models.contains(&"InvoiceSchema".to_string()));
        assert!(details.models.contains(&"PaymentValidator".to_string()));
    }

    #[test]
    fn test_ts_interface_and_class_detected() {
        let content = r#"
export interface Invoice {
    id: string;
    amount: number;
}

export class InvoiceService {
    create() {}
}

export enum Status {
    Active,
    Inactive,
}
"#;
        let mut details = DomainDetails::default();
        extract_models(content, "ts", &mut details);
        assert!(details.models.contains(&"Invoice".to_string()));
        assert!(details.models.contains(&"InvoiceService".to_string()));
        assert!(details.models.contains(&"Status".to_string()));
    }

    #[test]
    fn test_nestjs_routes_detected() {
        let content = r#"
@Controller('billing')
export class BillingController {
    @Get('invoices')
    findAll() {}

    @Post('invoices')
    create() {}
}
"#;
        let mut details = DomainDetails::default();
        let path = Path::new("src/billing/billing.controller.ts");
        extract_routes(content, "ts", path, &mut details);
        assert!(
            details
                .routes
                .contains(&"GET /billing/invoices".to_string()),
            "Expected NestJS GET route, found: {:?}",
            details.routes
        );
        assert!(
            details
                .routes
                .contains(&"POST /billing/invoices".to_string()),
            "Expected NestJS POST route, found: {:?}",
            details.routes
        );
    }

    #[test]
    fn test_nestjs_routes_without_controller_prefix() {
        let content = r#"
export class AppController {
    @Get('health')
    health() {}
}
"#;
        let mut details = DomainDetails::default();
        let path = Path::new("src/app.controller.ts");
        extract_routes(content, "ts", path, &mut details);
        assert!(
            details.routes.contains(&"GET health".to_string()),
            "Expected NestJS GET route without prefix, found: {:?}",
            details.routes
        );
    }

    #[test]
    fn test_express_routes_still_detected() {
        let content = r#"
router.get('/invoices', handler);
app.post('/payments', handler);
"#;
        let mut details = DomainDetails::default();
        let path = Path::new("src/routes/billing.ts");
        extract_routes(content, "ts", path, &mut details);
        assert!(
            details
                .routes
                .iter()
                .any(|r| r.contains("GET") && r.contains("/invoices"))
        );
        assert!(
            details
                .routes
                .iter()
                .any(|r| r.contains("POST") && r.contains("/payments"))
        );
    }

    #[test]
    fn test_comments_extracted() {
        let content = r#"
// TODO: refactor this function
// FIXME: handle edge case
/* NOTE: important business rule */
"#;
        let result = extract_file_details(content, Path::new("src/billing/invoice.ts"));
        assert_eq!(result.comments.len(), 3);
        assert!(result.comments.iter().any(|c| c.tag == "TODO"));
        assert!(result.comments.iter().any(|c| c.tag == "FIXME"));
        assert!(result.comments.iter().any(|c| c.tag == "NOTE"));
        // Verify file path is captured
        assert!(result
            .comments
            .iter()
            .all(|c| c.file_path == "src/billing/invoice.ts"));
    }

    #[test]
    fn test_vitest_patterns_in_comments() {
        // Test files themselves are detected by structure::is_test_file,
        // but comments in tests are also captured
        let content = r#"
// TODO: add edge case tests for billing
describe('Invoice', () => {
    it('should calculate total', () => {
        // NOTE: uses legacy calculation for client X
    });
});
"#;
        let result = extract_file_details(content, Path::new("tests/billing.test.ts"));
        assert!(result.comments.iter().any(|c| c.tag == "TODO"));
        assert!(result.comments.iter().any(|c| c.tag == "NOTE"));
    }
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
