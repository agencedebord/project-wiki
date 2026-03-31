use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::scan::DomainInfo;
use crate::ui;

// ─── Constants ───

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_RETRIES: u32 = 3;
const BASE_RETRY_DELAY_MS: u64 = 1000;
const MAX_FILE_SNIPPETS: usize = 5;
const SNIPPET_LINE_LIMIT: usize = 50;
const MAX_TOKENS_RESPONSE: u32 = 1024;

// ─── Types ───

#[derive(Debug, Deserialize)]
struct EnrichmentResponse {
    description: String,
    #[serde(default)]
    key_behaviors: Vec<String>,
    #[serde(default)]
    memory_candidates: Vec<LlmCandidate>,
    #[serde(default)]
    contradictions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LlmCandidate {
    #[serde(rename = "type")]
    type_: String,
    text: String,
}

struct FileSnippet {
    path: String,
    content: String,
}

// ─── Public entry point ───

/// Enrich all domains with LLM suggestions.
/// Reads existing _overview.md files and appends LLM sections.
/// Errors on individual domains are logged as warnings, never abort.
pub async fn run(domains: &[DomainInfo], wiki_dir: &Path) -> Result<()> {
    let token = resolve_token(wiki_dir)?;
    let client = reqwest::Client::new();

    let total = domains.len();
    for (i, domain) in domains.iter().enumerate() {
        let progress = (i + 1) as f64 / total as f64;
        ui::enrich_progress(&format!("Enriching {}...", domain.name), progress);

        match enrich_domain(&client, &token, domain).await {
            Ok(response) => {
                if let Err(e) = inject_enrichment(wiki_dir, &domain.name, &response) {
                    ui::warn(&format!(
                        "Failed to write enrichment for {}: {}",
                        domain.name, e
                    ));
                }
            }
            Err(e) => {
                // Auth failures should stop all processing
                let err_str = format!("{}", e);
                if err_str.contains("authentication failed") {
                    return Err(e);
                }
                ui::warn(&format!(
                    "Failed to enrich {}: {}. Skipping.",
                    domain.name, e
                ));
            }
        }
    }

    ui::success(&format!("LLM enrichment complete for {} domain(s).", total));

    Ok(())
}

// ─── Token resolution ───

fn resolve_token(wiki_dir: &Path) -> Result<String> {
    // 1. Check ANTHROPIC_API_KEY env var (standard convention)
    if let Ok(token) = std::env::var("ANTHROPIC_API_KEY") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    // 2. Check wiki/.env file for WIKI_LLM_KEY
    if let Some(token) = read_token_from_env_file(wiki_dir, "WIKI_LLM_KEY") {
        return Ok(token);
    }

    // 3. Interactive prompt
    eprintln!();
    eprintln!(
        "{} Anthropic API key not found.",
        console::style("◆").cyan()
    );
    eprintln!(
        "{} Set ANTHROPIC_API_KEY env var or add WIKI_LLM_KEY to .wiki/.env",
        console::style("│").dim()
    );
    eprintln!();

    let token: String = dialoguer::Input::new()
        .with_prompt("Enter your Anthropic API key")
        .interact_text()
        .context("Failed to read API key")?;

    if token.is_empty() {
        bail!("Anthropic API key is required for --enrich");
    }

    // 4. Offer to save
    if let Ok(save) = dialoguer::Confirm::new()
        .with_prompt("Save key to .wiki/.env as WIKI_LLM_KEY?")
        .default(true)
        .interact()
    {
        if save {
            save_token_to_env(wiki_dir, &token)?;
        }
    }

    Ok(token)
}

fn read_token_from_env_file(wiki_dir: &Path, key: &str) -> Option<String> {
    let env_path = wiki_dir.join(".env");
    let file = std::fs::File::open(&env_path).ok()?;
    let reader = BufReader::new(file);
    let prefix = format!("{}=", key);

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(&prefix) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn save_token_to_env(wiki_dir: &Path, token: &str) -> Result<()> {
    let env_path = wiki_dir.join(".env");
    let mut content = if env_path.exists() {
        std::fs::read_to_string(&env_path).unwrap_or_default()
    } else {
        String::new()
    };

    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&format!("WIKI_LLM_KEY={}\n", token));

    std::fs::write(&env_path, content).context("Failed to write .wiki/.env")?;
    ui::info("API key saved to .wiki/.env");
    Ok(())
}

// ─── Domain enrichment ───

async fn enrich_domain(
    client: &reqwest::Client,
    token: &str,
    domain: &DomainInfo,
) -> Result<EnrichmentResponse> {
    let snippets = collect_file_snippets(domain);
    let prompt = build_prompt(domain, &snippets);
    let mut response = call_anthropic(client, token, &prompt).await?;
    validate_response(&mut response);
    Ok(response)
}

fn collect_file_snippets(domain: &DomainInfo) -> Vec<FileSnippet> {
    let mut candidates: Vec<&String> = domain
        .files
        .iter()
        .filter(|f| !domain.test_files.contains(f))
        .collect();

    // Sort: files with model/route-related names first, then by path length (shorter = more central)
    candidates.sort_by(|a, b| {
        let a_score = file_priority_score(a, domain);
        let b_score = file_priority_score(b, domain);
        b_score.cmp(&a_score).then(a.len().cmp(&b.len()))
    });

    candidates
        .into_iter()
        .take(MAX_FILE_SNIPPETS)
        .filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
            let lines: Vec<&str> = content.lines().take(SNIPPET_LINE_LIMIT).collect();
            if lines.is_empty() {
                return None;
            }
            Some(FileSnippet {
                path: path.clone(),
                content: lines.join("\n"),
            })
        })
        .collect()
}

/// Score a file for snippet priority (higher = more important).
fn file_priority_score(path: &str, domain: &DomainInfo) -> u32 {
    let lower = path.to_lowercase();
    let mut score = 0;

    // Files whose name matches a model name
    for model in &domain.models {
        if lower.contains(&model.to_lowercase()) {
            score += 3;
            break;
        }
    }

    // Files with route/controller/handler in name
    if lower.contains("route")
        || lower.contains("controller")
        || lower.contains("handler")
        || lower.contains("api")
    {
        score += 2;
    }

    // Module entry points
    if lower.ends_with("mod.rs")
        || lower.ends_with("index.ts")
        || lower.ends_with("index.js")
        || lower.ends_with("__init__.py")
    {
        score += 1;
    }

    score
}

fn build_prompt(domain: &DomainInfo, snippets: &[FileSnippet]) -> String {
    let models_str = if domain.models.is_empty() {
        "none detected".to_string()
    } else {
        domain.models.join(", ")
    };

    let routes_str = if domain.routes.is_empty() {
        "none detected".to_string()
    } else {
        domain.routes.join(", ")
    };

    let deps_str = if domain.dependencies.is_empty() {
        "none".to_string()
    } else {
        domain.dependencies.join(", ")
    };

    let comments_str = if domain.comments.is_empty() {
        "none".to_string()
    } else {
        domain.comments.join("; ")
    };

    let tests_str = if domain.test_files.is_empty() {
        "none".to_string()
    } else {
        domain.test_files.join(", ")
    };

    let snippets_str = if snippets.is_empty() {
        "No source code available.".to_string()
    } else {
        snippets
            .iter()
            .map(|s| format!("### {}\n```\n{}\n```", s.path, s.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    format!(
        r#"You are analyzing a software domain called "{domain}" to propose documentation suggestions.

## Domain metadata

- **Structural summary**: {description}
- **Models**: {models}
- **API routes**: {routes}
- **Dependencies**: {deps}
- **Test files**: {tests}
- **Code comments**: {comments}

## Source file excerpts

{snippets}

## Instructions

Based on this information, propose:
1. A **description** (1-2 factual sentences about what this domain does)
2. **key_behaviors** (2-3 key behaviors this domain implements — phrase as observations, not assertions)
3. Optionally, **memory_candidates** (0-3 items) — business rules, exceptions, or decisions you can infer. Each must have a "type" (exception|decision|business_rule) and "text".
4. Optionally, **contradictions** — anything that seems unclear or contradictory.

Rules:
- Be factual. Only propose what the code evidence supports.
- Do NOT invent capabilities not visible in the metadata or code.
- If there is not enough signal for a field, return an empty array.
- Prefer short, precise language.

Respond with ONLY a JSON object (no markdown fencing):
{{"description": "...", "key_behaviors": ["..."], "memory_candidates": [{{"type": "...", "text": "..."}}], "contradictions": ["..."]}}"#,
        domain = domain.name,
        description = domain.structural_description(),
        models = models_str,
        routes = routes_str,
        deps = deps_str,
        tests = tests_str,
        comments = comments_str,
        snippets = snippets_str,
    )
}

// ─── Anthropic API call ───

async fn call_anthropic(
    client: &reqwest::Client,
    token: &str,
    prompt: &str,
) -> Result<EnrichmentResponse> {
    let body = serde_json::json!({
        "model": ANTHROPIC_MODEL,
        "max_tokens": MAX_TOKENS_RESPONSE,
        "messages": [{
            "role": "user",
            "content": prompt
        }]
    });

    let mut retries = 0;

    loop {
        let resp = client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", token)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let api_resp: serde_json::Value = r
                    .json()
                    .await
                    .context("Failed to parse Anthropic API response")?;

                let text = api_resp["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Unexpected API response structure"))?;

                // Strip markdown fencing if present
                let clean = strip_json_fencing(text);

                let parsed: EnrichmentResponse = serde_json::from_str(clean)
                    .with_context(|| format!("Failed to parse LLM response as JSON: {}", clean))?;

                return Ok(parsed);
            }
            Ok(r) if r.status().as_u16() == 429 => {
                retries += 1;
                if retries > MAX_RETRIES {
                    bail!("Anthropic API rate limited after {} retries", MAX_RETRIES);
                }
                let delay = Duration::from_millis(BASE_RETRY_DELAY_MS * 2u64.pow(retries));
                ui::warn(&format!(
                    "Rate limited. Retrying in {:?}... ({}/{})",
                    delay, retries, MAX_RETRIES
                ));
                tokio::time::sleep(delay).await;
            }
            Ok(r) if r.status().as_u16() == 401 => {
                bail!(
                    "Anthropic API authentication failed. Check your ANTHROPIC_API_KEY or WIKI_LLM_KEY."
                );
            }
            Ok(r) => {
                let status = r.status();
                let error_body = r.text().await.unwrap_or_default();
                bail!("Anthropic API error ({}): {}", status, error_body);
            }
            Err(e) => {
                retries += 1;
                if retries > MAX_RETRIES {
                    bail!("Network error after {} retries: {}", MAX_RETRIES, e);
                }
                let delay = Duration::from_millis(BASE_RETRY_DELAY_MS * 2u64.pow(retries));
                ui::warn(&format!(
                    "Network error. Retrying in {:?}... ({}/{})",
                    delay, retries, MAX_RETRIES
                ));
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Strip markdown JSON fencing (```json ... ```) if the LLM adds it.
fn strip_json_fencing(text: &str) -> &str {
    let trimmed = text.trim();

    // Try ```json ... ```
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }

    // Try ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }

    trimmed
}

// ─── Response validation ───

fn validate_response(resp: &mut EnrichmentResponse) {
    // Cap key_behaviors at 3
    resp.key_behaviors.truncate(3);

    // Cap memory_candidates at 3 and filter invalid types
    resp.memory_candidates
        .retain(|c| matches!(c.type_.as_str(), "exception" | "decision" | "business_rule"));
    resp.memory_candidates.truncate(3);

    // Truncate overly long descriptions
    if resp.description.len() > 300 {
        resp.description.truncate(297);
        resp.description.push_str("...");
    }
}

// ─── Injection into _overview.md ───

fn inject_enrichment(
    wiki_dir: &Path,
    domain_name: &str,
    response: &EnrichmentResponse,
) -> Result<()> {
    let overview_path = wiki_dir
        .join("domains")
        .join(domain_name)
        .join("_overview.md");

    if !overview_path.exists() {
        return Ok(());
    }

    let mut content = std::fs::read_to_string(&overview_path)
        .with_context(|| format!("Failed to read {}", overview_path.display()))?;

    // Append LLM suggestion sections
    content.push_str("\n\n## Description (LLM suggestion)\n");
    content.push_str(&format!("{} `[llm-suggestion]`\n", response.description));

    if !response.key_behaviors.is_empty() {
        content.push_str("\n## Key behaviors (LLM suggestion)\n");
        for behavior in &response.key_behaviors {
            content.push_str(&format!("- {} `[llm-suggestion]`\n", behavior));
        }
    }

    if !response.memory_candidates.is_empty() {
        content.push_str("\n## Memory candidates (LLM suggestion)\n");
        for candidate in &response.memory_candidates {
            content.push_str(&format!(
                "- [{}] {} `[llm-suggestion]`\n",
                candidate.type_, candidate.text
            ));
        }
    }

    if !response.contradictions.is_empty() {
        content.push_str("\n## Unclear areas (LLM suggestion)\n");
        for item in &response.contradictions {
            content.push_str(&format!("- {} `[llm-suggestion]`\n", item));
        }
    }

    std::fs::write(&overview_path, content)
        .with_context(|| format!("Failed to write {}", overview_path.display()))?;

    Ok(())
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_json_fencing_plain() {
        let input = r#"{"description": "test"}"#;
        assert_eq!(strip_json_fencing(input), input);
    }

    #[test]
    fn strip_json_fencing_with_json_tag() {
        let input = "```json\n{\"description\": \"test\"}\n```";
        assert_eq!(strip_json_fencing(input), "{\"description\": \"test\"}");
    }

    #[test]
    fn strip_json_fencing_with_bare_backticks() {
        let input = "```\n{\"description\": \"test\"}\n```";
        assert_eq!(strip_json_fencing(input), "{\"description\": \"test\"}");
    }

    #[test]
    fn validate_response_truncates() {
        let mut resp = EnrichmentResponse {
            description: "x".repeat(400),
            key_behaviors: vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ],
            memory_candidates: vec![
                LlmCandidate {
                    type_: "decision".to_string(),
                    text: "ok".to_string(),
                },
                LlmCandidate {
                    type_: "invalid_type".to_string(),
                    text: "bad".to_string(),
                },
            ],
            contradictions: vec![],
        };

        validate_response(&mut resp);

        assert_eq!(resp.description.len(), 300);
        assert!(resp.description.ends_with("..."));
        assert_eq!(resp.key_behaviors.len(), 3);
        assert_eq!(resp.memory_candidates.len(), 1);
        assert_eq!(resp.memory_candidates[0].type_, "decision");
    }

    #[test]
    fn file_priority_score_model_match() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };

        assert!(file_priority_score("src/billing/invoice.rs", &domain) >= 3);
        assert_eq!(file_priority_score("src/billing/utils.rs", &domain), 0);
    }

    #[test]
    fn file_priority_score_route_patterns() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };

        assert!(file_priority_score("src/billing/controller.ts", &domain) >= 2);
        assert!(file_priority_score("src/billing/handler.rs", &domain) >= 2);
        assert!(file_priority_score("src/api/billing.ts", &domain) >= 2);
    }

    #[test]
    fn parse_enrichment_response() {
        let json = r#"{
            "description": "Handles billing operations",
            "key_behaviors": ["Processes invoices", "Validates payments"],
            "memory_candidates": [{"type": "business_rule", "text": "Invoices expire after 30 days"}],
            "contradictions": []
        }"#;

        let resp: EnrichmentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.description, "Handles billing operations");
        assert_eq!(resp.key_behaviors.len(), 2);
        assert_eq!(resp.memory_candidates.len(), 1);
        assert_eq!(resp.memory_candidates[0].type_, "business_rule");
    }

    #[test]
    fn parse_enrichment_response_minimal() {
        let json = r#"{"description": "A simple domain"}"#;
        let resp: EnrichmentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.description, "A simple domain");
        assert!(resp.key_behaviors.is_empty());
        assert!(resp.memory_candidates.is_empty());
        assert!(resp.contradictions.is_empty());
    }
}
