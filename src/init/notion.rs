use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::ui;

// ─── Public types ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionTicket {
    pub id: String,
    pub title: String,
    pub status: Option<String>,
    pub date: Option<String>,
    pub tags: Vec<String>,
    pub description: String,
    pub comments: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanProgress {
    pub notion_url: String,
    pub total_tickets: usize,
    pub last_cursor: Option<String>,
    pub tickets_processed: usize,
    pub batch_size: usize,
    pub domains_mapped: HashMap<String, Vec<String>>,
    pub started_at: String,
}

#[derive(Debug)]
pub struct NotionDomainInfo {
    pub name: String,
    pub tickets: Vec<NotionTicket>,
    pub business_rules: Vec<String>,
    pub decisions: Vec<String>,
    pub contradictions: Vec<(String, String)>,
}

// ─── Constants ───

const NOTION_API_BASE: &str = "https://api.notion.so/v1";
const NOTION_VERSION: &str = "2022-06-28";
const BATCH_SIZE: usize = 100;
const MAX_RETRIES: u32 = 5;
const BASE_RETRY_DELAY_MS: u64 = 1000;
const DETAIL_CONCURRENCY: usize = 5;

// ─── Main entry point ───

pub async fn run(notion_url: &str, resume: bool, wiki_dir: &Path) -> Result<Vec<NotionDomainInfo>> {
    ui::action("Importing from Notion database");
    eprintln!();

    // 1. Resolve token
    let token = resolve_token(wiki_dir)?;

    // 2. Extract database ID from URL
    let db_id = parse_notion_url(notion_url)?;
    ui::step(&format!("Database ID: {}", db_id));

    // 3. Check for resume
    let progress_path = wiki_dir.join(".scan-progress.json");
    let cache_path = wiki_dir.join(".notion-cache.json");
    let mut progress = if resume {
        load_progress(&progress_path)?
    } else {
        None
    };

    // Load cached tickets from previous runs (for resume)
    let mut cached_tickets: HashMap<String, NotionTicket> = if resume {
        load_ticket_cache(&cache_path).unwrap_or_default()
    } else {
        HashMap::new()
    };

    // 4. Pass 1 — Inventory (paginated)
    ui::step("Pass 1: Fetching ticket inventory...");
    let tickets = fetch_all_tickets(&token, &db_id, &mut progress, &progress_path).await?;
    ui::info(&format!("  Found {} ticket(s)", tickets.len()));

    // 5. Pass 2 — Detail extraction (concurrent, with cache)
    ui::step("Pass 2: Extracting ticket details...");
    let tickets_to_fetch: Vec<&NotionTicket> = tickets
        .iter()
        .filter(|t| !cached_tickets.contains_key(&t.id))
        .collect();

    if !cached_tickets.is_empty() && !tickets_to_fetch.is_empty() {
        ui::info(&format!(
            "  {} ticket(s) already cached, fetching {} remaining",
            tickets.len() - tickets_to_fetch.len(),
            tickets_to_fetch.len()
        ));
    }

    let newly_fetched = fetch_ticket_details(&token, &tickets_to_fetch, &cache_path).await?;

    // Merge newly fetched into cache
    for ticket in &newly_fetched {
        cached_tickets.insert(ticket.id.clone(), ticket.clone());
    }

    // Build final enriched list in original order
    let enriched_tickets: Vec<NotionTicket> = tickets
        .iter()
        .map(|t| {
            cached_tickets
                .get(&t.id)
                .cloned()
                .unwrap_or_else(|| t.clone())
        })
        .collect();

    // 6. Collect existing domain names from wiki
    let existing_domains = collect_existing_domains(wiki_dir);

    // 7. Map tickets to domains
    let mut domain_map: HashMap<String, Vec<NotionTicket>> = HashMap::new();
    for ticket in &enriched_tickets {
        let domain = map_ticket_to_domain(ticket, &existing_domains);
        domain_map.entry(domain).or_default().push(ticket.clone());
    }

    // 8. Pass 3 — Contradiction detection
    ui::step("Pass 3: Detecting contradictions...");
    let mut result = Vec::new();
    for (domain_name, tickets) in &domain_map {
        let contradictions = detect_contradictions(tickets);
        let business_rules = extract_business_rules(tickets);
        let decisions = extract_decisions(tickets);

        if !contradictions.is_empty() {
            ui::warn(&format!(
                "  {} contradiction(s) in domain \"{}\"",
                contradictions.len(),
                domain_name
            ));
        }

        result.push(NotionDomainInfo {
            name: domain_name.clone(),
            tickets: tickets.clone(),
            business_rules,
            decisions,
            contradictions,
        });
    }

    // Clean up progress and cache files
    if progress_path.exists() {
        let _ = fs::remove_file(&progress_path);
    }
    if cache_path.exists() {
        let _ = fs::remove_file(&cache_path);
    }

    ui::success(&format!(
        "Imported {} ticket(s) across {} domain(s)",
        enriched_tickets.len(),
        result.len()
    ));

    Ok(result)
}

// ─── Token resolution ───

fn resolve_token(wiki_dir: &Path) -> Result<String> {
    // 1. Check env var
    if let Ok(token) = std::env::var("WIKI_NOTION_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    // 2. Check wiki/.env file
    if let Some(token) = read_token_from_env_file(wiki_dir) {
        return Ok(token);
    }

    // 3. Interactive prompt
    let token = prompt_for_token()?;

    // 4. Offer to save
    if let Ok(save) = dialoguer::Confirm::new()
        .with_prompt("Save token to wiki/.env?")
        .default(true)
        .interact()
    {
        if save {
            save_token_to_env(wiki_dir, &token)?;
        }
    }

    Ok(token)
}

fn read_token_from_env_file(wiki_dir: &Path) -> Option<String> {
    let env_path = wiki_dir.join(".env");
    let file = fs::File::open(&env_path).ok()?;
    let reader = BufReader::new(file);

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("WIKI_NOTION_TOKEN=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn prompt_for_token() -> Result<String> {
    eprintln!();
    eprintln!("{} Notion API token not found.", console::style("◆").cyan());
    eprintln!(
        "{} Create an integration at https://www.notion.so/my-integrations",
        console::style("│").dim()
    );
    eprintln!(
        "{} Then share your database with the integration.",
        console::style("│").dim()
    );
    eprintln!();

    let token: String = dialoguer::Input::new()
        .with_prompt("Enter your Notion API token")
        .interact_text()
        .context("Failed to read Notion token")?;

    if token.is_empty() {
        bail!("Notion API token is required for --from-notion");
    }

    Ok(token)
}

fn save_token_to_env(wiki_dir: &Path, token: &str) -> Result<()> {
    let env_path = wiki_dir.join(".env");
    let mut content = if env_path.exists() {
        fs::read_to_string(&env_path).unwrap_or_default()
    } else {
        String::new()
    };

    // Check if the key already exists and update it
    if content.contains("WIKI_NOTION_TOKEN=") {
        let lines: Vec<String> = content
            .lines()
            .map(|line| {
                if line.trim().starts_with("WIKI_NOTION_TOKEN=") {
                    format!("WIKI_NOTION_TOKEN={}", token)
                } else {
                    line.to_string()
                }
            })
            .collect();
        content = lines.join("\n");
        if !content.ends_with('\n') {
            content.push('\n');
        }
    } else {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("WIKI_NOTION_TOKEN={}\n", token));
    }

    fs::write(&env_path, content).context("Failed to write .env file")?;
    ui::info("Token saved to .wiki/.env");
    Ok(())
}

// ─── URL parsing ───

pub fn parse_notion_url(url: &str) -> Result<String> {
    // Handle formats:
    // https://www.notion.so/workspace/HEXID?v=...
    // https://www.notion.so/workspace/HEXID
    // https://notion.so/HEXID?v=...
    // Just a bare hex ID or UUID

    let cleaned = url.trim();

    // If it's already a UUID with dashes
    if is_uuid(cleaned) {
        return Ok(cleaned.to_string());
    }

    // If it's a 32-char hex string
    if is_hex_id(cleaned) {
        return Ok(format_uuid_from_hex(cleaned));
    }

    // Parse as URL
    let parts: Vec<&str> = cleaned
        .split('?')
        .next()
        .unwrap_or(cleaned)
        .split('/')
        .collect();

    // Find the last path segment that looks like a hex ID
    for part in parts.iter().rev() {
        // Notion URLs sometimes have the title prepended with a dash: "My-Page-HEXID"
        // The hex ID is always the last 32 chars
        let segment = *part;

        if segment.len() >= 32 {
            let potential_hex = &segment[segment.len() - 32..];
            if is_hex_id(potential_hex) {
                return Ok(format_uuid_from_hex(potential_hex));
            }
        }

        if is_hex_id(segment) {
            return Ok(format_uuid_from_hex(segment));
        }

        if is_uuid(segment) {
            return Ok(segment.to_string());
        }
    }

    bail!(
        "Could not extract a database ID from the Notion URL: {}",
        url
    );
}

pub fn format_uuid_from_hex(hex: &str) -> String {
    // 32 hex chars → UUID with dashes: 8-4-4-4-12
    let h = hex.replace('-', "");
    if h.len() != 32 {
        return hex.to_string();
    }
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn is_hex_id(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && s.matches('-').count() == 4
}

// ─── Notion API calls ───

async fn fetch_all_tickets(
    token: &str,
    database_id: &str,
    progress: &mut Option<ScanProgress>,
    progress_path: &Path,
) -> Result<Vec<NotionTicket>> {
    let client = reqwest::Client::new();
    let url = format!("{}/databases/{}/query", NOTION_API_BASE, database_id);

    let mut all_tickets: Vec<NotionTicket> = Vec::new();
    let mut cursor: Option<String> = progress.as_ref().and_then(|p| p.last_cursor.clone());
    let mut has_more = true;
    let mut page_count = 0u32;

    while has_more {
        let mut body = serde_json::json!({
            "page_size": BATCH_SIZE,
        });

        if let Some(ref c) = cursor {
            body["start_cursor"] = serde_json::Value::String(c.clone());
        }

        let response = api_request_with_retry(&client, &url, token, Some(&body)).await?;

        let results = response["results"]
            .as_array()
            .context("Invalid response: missing results array")?;

        for item in results {
            if let Some(ticket) = parse_ticket_from_page(item) {
                all_tickets.push(ticket);
            }
        }

        has_more = response["has_more"].as_bool().unwrap_or(false);
        cursor = response["next_cursor"].as_str().map(|s| s.to_string());

        page_count += 1;

        // Show progress
        let total_estimate = if has_more {
            all_tickets.len() + BATCH_SIZE // rough estimate
        } else {
            all_tickets.len()
        };
        let prog = if total_estimate > 0 {
            all_tickets.len() as f64 / total_estimate as f64
        } else {
            1.0
        };
        ui::notion_progress(
            &format!(
                "Fetched {} tickets (page {})",
                all_tickets.len(),
                page_count
            ),
            prog.min(0.99),
        );

        // Save progress for resume
        let scan_progress = ScanProgress {
            notion_url: database_id.to_string(),
            total_tickets: all_tickets.len(),
            last_cursor: cursor.clone(),
            tickets_processed: all_tickets.len(),
            batch_size: BATCH_SIZE,
            domains_mapped: HashMap::new(),
            started_at: progress
                .as_ref()
                .map(|p| p.started_at.clone())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        };

        if let Ok(json) = serde_json::to_string_pretty(&scan_progress) {
            let _ = fs::write(progress_path, json);
        }

        *progress = Some(scan_progress);
    }

    ui::notion_progress(
        &format!("Inventory complete: {} tickets", all_tickets.len()),
        1.0,
    );

    Ok(all_tickets)
}

fn parse_ticket_from_page(page: &serde_json::Value) -> Option<NotionTicket> {
    let id = page["id"].as_str()?.to_string();
    let properties = &page["properties"];

    // Extract title — try common property names
    let title = extract_title_property(properties).unwrap_or_else(|| "Untitled".to_string());

    // Extract status
    let status = extract_status_property(properties);

    // Extract date
    let date = extract_date_property(properties);

    // Extract tags
    let tags = extract_tags_property(properties);

    Some(NotionTicket {
        id,
        title,
        status,
        date,
        tags,
        description: String::new(), // filled in pass 2
        comments: Vec::new(),       // filled in pass 2
    })
}

fn extract_title_property(properties: &serde_json::Value) -> Option<String> {
    // Notion stores title in a property of type "title"
    if let Some(obj) = properties.as_object() {
        for (_key, value) in obj {
            if value["type"].as_str() == Some("title") {
                if let Some(title_arr) = value["title"].as_array() {
                    let text: String = title_arr
                        .iter()
                        .filter_map(|t| t["plain_text"].as_str())
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

fn extract_status_property(properties: &serde_json::Value) -> Option<String> {
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            let key_lower = key.to_lowercase();
            if key_lower == "status" || key_lower == "state" || key_lower == "statut" {
                // Status type
                if let Some(name) = value["status"]["name"].as_str() {
                    return Some(name.to_string());
                }
                // Select type
                if let Some(name) = value["select"]["name"].as_str() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

fn extract_date_property(properties: &serde_json::Value) -> Option<String> {
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            let key_lower = key.to_lowercase();
            if key_lower == "date"
                || key_lower == "created"
                || key_lower == "due"
                || key_lower == "due date"
            {
                if let Some(start) = value["date"]["start"].as_str() {
                    return Some(start.to_string());
                }
            }
        }
        // Fall back to created_time on the page itself
    }
    None
}

fn extract_tags_property(properties: &serde_json::Value) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            let key_lower = key.to_lowercase();
            if key_lower == "tags"
                || key_lower == "labels"
                || key_lower == "category"
                || key_lower == "type"
            {
                // Multi-select
                if let Some(arr) = value["multi_select"].as_array() {
                    for item in arr {
                        if let Some(name) = item["name"].as_str() {
                            tags.push(name.to_string());
                        }
                    }
                }
                // Single select
                if let Some(name) = value["select"]["name"].as_str() {
                    tags.push(name.to_string());
                }
            }
        }
    }
    tags
}

// ─── Pass 2: Detail extraction ───

async fn fetch_ticket_details(
    token: &str,
    tickets: &[&NotionTicket],
    cache_path: &Path,
) -> Result<Vec<NotionTicket>> {
    let total = tickets.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let semaphore = Arc::new(Semaphore::new(DETAIL_CONCURRENCY));
    let token = token.to_string();
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let futures = tickets.iter().map(|ticket| {
        let client = client.clone();
        let token = token.clone();
        let semaphore = semaphore.clone();
        let completed = completed.clone();
        let ticket = (*ticket).clone();

        async move {
            let _permit = semaphore
                .acquire()
                .await
                .expect("semaphore closed unexpectedly");

            let mut enriched_ticket = ticket.clone();

            // Fetch blocks (content)
            let blocks_url = format!(
                "{}/blocks/{}/children?page_size=100",
                NOTION_API_BASE, ticket.id
            );
            if let Ok(response) = api_get_with_retry(&client, &blocks_url, &token).await {
                if let Some(results) = response["results"].as_array() {
                    enriched_ticket.description = extract_text_from_blocks(results);
                }
            }

            // Fetch comments
            let comments_url = format!(
                "{}/comments?block_id={}&page_size=100",
                NOTION_API_BASE, ticket.id
            );
            if let Ok(response) = api_get_with_retry(&client, &comments_url, &token).await {
                if let Some(results) = response["results"].as_array() {
                    enriched_ticket.comments = extract_comments(results);
                }
            }

            let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if done % 5 == 0 || done == total {
                let prog = done as f64 / total as f64;
                ui::notion_progress(&format!("Details: {}/{} tickets", done, total), prog);
            }

            enriched_ticket
        }
    });

    let enriched: Vec<NotionTicket> = stream::iter(futures)
        .buffer_unordered(DETAIL_CONCURRENCY)
        .collect()
        .await;

    // Save to cache after each batch completes
    save_ticket_cache(cache_path, &enriched);

    Ok(enriched)
}

pub fn extract_text_from_blocks(blocks: &[serde_json::Value]) -> String {
    let mut text_parts = Vec::new();

    for block in blocks {
        let block_type = match block["type"].as_str() {
            Some(t) => t,
            None => continue,
        };

        let rich_text = match block_type {
            "paragraph" => block["paragraph"]["rich_text"].as_array(),
            "heading_1" => block["heading_1"]["rich_text"].as_array(),
            "heading_2" => block["heading_2"]["rich_text"].as_array(),
            "heading_3" => block["heading_3"]["rich_text"].as_array(),
            "bulleted_list_item" => block["bulleted_list_item"]["rich_text"].as_array(),
            "numbered_list_item" => block["numbered_list_item"]["rich_text"].as_array(),
            "to_do" => block["to_do"]["rich_text"].as_array(),
            "quote" => block["quote"]["rich_text"].as_array(),
            "callout" => block["callout"]["rich_text"].as_array(),
            "toggle" => block["toggle"]["rich_text"].as_array(),
            _ => None,
        };

        if let Some(rich_text_arr) = rich_text {
            let line: String = rich_text_arr
                .iter()
                .filter_map(|rt| rt["plain_text"].as_str())
                .collect::<Vec<_>>()
                .join("");

            if !line.is_empty() {
                let prefix = match block_type {
                    "heading_1" => "# ",
                    "heading_2" => "## ",
                    "heading_3" => "### ",
                    "bulleted_list_item" => "- ",
                    "numbered_list_item" => "1. ",
                    "to_do" => {
                        let checked = block["to_do"]["checked"].as_bool().unwrap_or(false);
                        if checked { "- [x] " } else { "- [ ] " }
                    }
                    "quote" => "> ",
                    _ => "",
                };
                text_parts.push(format!("{}{}", prefix, line));
            }
        }
    }

    text_parts.join("\n")
}

fn extract_comments(results: &[serde_json::Value]) -> Vec<String> {
    let mut comments = Vec::new();
    for comment in results {
        if let Some(rich_text) = comment["rich_text"].as_array() {
            let text: String = rich_text
                .iter()
                .filter_map(|rt| rt["plain_text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            if !text.is_empty() {
                comments.push(text);
            }
        }
    }
    comments
}

// ─── Domain mapping ───

pub fn map_ticket_to_domain(ticket: &NotionTicket, existing_domains: &[String]) -> String {
    // 1. Check tags against domain names
    for tag in &ticket.tags {
        let tag_lower = tag.to_lowercase().replace([' ', '_'], "-");
        for domain in existing_domains {
            if tag_lower == *domain
                || tag_lower.contains(domain.as_str())
                || domain.contains(&tag_lower)
            {
                return domain.clone();
            }
        }
    }

    // 2. Check title keywords against domain names
    let title_lower = ticket.title.to_lowercase();
    for domain in existing_domains {
        if title_lower.contains(domain.as_str()) {
            return domain.clone();
        }
    }

    // 3. Use the first tag as domain name (normalized)
    if let Some(tag) = ticket.tags.first() {
        return tag.to_lowercase().replace([' ', '_'], "-");
    }

    // 4. Fall back to uncategorized
    "uncategorized".to_string()
}

fn collect_existing_domains(wiki_dir: &Path) -> Vec<String> {
    let domains_dir = wiki_dir.join("domains");
    if !domains_dir.exists() {
        return Vec::new();
    }

    fs::read_dir(&domains_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    e.file_name()
                        .to_str()
                        .filter(|n| *n != ".gitkeep")
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

// ─── Contradiction detection ───

pub fn detect_contradictions(tickets: &[NotionTicket]) -> Vec<(String, String)> {
    let mut contradictions = Vec::new();

    // Sort tickets by date (newest first)
    let mut sorted = tickets.to_vec();
    sorted.sort_by(|a, b| {
        let date_a = a.date.as_deref().unwrap_or("");
        let date_b = b.date.as_deref().unwrap_or("");
        date_b.cmp(date_a)
    });

    // Opposing action pairs
    let opposites: &[(&str, &str)] = &[
        ("add", "remove"),
        ("enable", "disable"),
        ("create", "delete"),
        ("activate", "deactivate"),
        ("show", "hide"),
        ("allow", "block"),
        ("allow", "deny"),
        ("open", "close"),
        ("increase", "decrease"),
        ("start", "stop"),
    ];

    // Compare each pair of tickets
    for i in 0..sorted.len() {
        for j in (i + 1)..sorted.len() {
            let a = &sorted[i];
            let b = &sorted[j];

            let title_a = a.title.to_lowercase();
            let title_b = b.title.to_lowercase();

            for (word1, word2) in opposites {
                let a_has_w1 = title_a.contains(word1);
                let a_has_w2 = title_a.contains(word2);
                let b_has_w1 = title_b.contains(word1);
                let b_has_w2 = title_b.contains(word2);

                // Check if they have opposite actions on similar subjects
                if (a_has_w1 && b_has_w2) || (a_has_w2 && b_has_w1) {
                    // Check if they share a common subject word (excluding the action itself)
                    let a_words: Vec<&str> = title_a
                        .split_whitespace()
                        .filter(|w| w != word1 && w != word2)
                        .collect();
                    let b_words: Vec<&str> = title_b
                        .split_whitespace()
                        .filter(|w| w != word1 && w != word2)
                        .collect();

                    let has_common_subject =
                        a_words.iter().any(|w| b_words.contains(w) && w.len() > 2);

                    if has_common_subject {
                        contradictions.push((a.title.clone(), b.title.clone()));
                    }
                }
            }
        }
    }

    contradictions
}

// ─── Business rule and decision extraction ───

fn extract_business_rules(tickets: &[NotionTicket]) -> Vec<String> {
    let mut rules = Vec::new();
    let rule_indicators = [
        "must",
        "should",
        "always",
        "never",
        "required",
        "mandatory",
        "rule:",
        "constraint:",
        "policy:",
    ];

    for ticket in tickets {
        // Check title
        let title_lower = ticket.title.to_lowercase();
        if rule_indicators.iter().any(|ind| title_lower.contains(ind)) {
            rules.push(ticket.title.clone());
        }

        // Check description lines
        for line in ticket.description.lines() {
            let line_lower = line.to_lowercase();
            if rule_indicators.iter().any(|ind| line_lower.contains(ind)) && line.len() > 10 {
                rules.push(line.trim().to_string());
            }
        }
    }

    // Deduplicate
    rules.sort();
    rules.dedup();
    rules
}

fn extract_decisions(tickets: &[NotionTicket]) -> Vec<String> {
    let mut decisions = Vec::new();
    let decision_indicators = [
        "decided",
        "decision:",
        "we chose",
        "we decided",
        "go with",
        "approved",
        "agreed",
        "selected",
    ];

    for ticket in tickets {
        let title_lower = ticket.title.to_lowercase();
        if decision_indicators
            .iter()
            .any(|ind| title_lower.contains(ind))
        {
            decisions.push(ticket.title.clone());
        }

        // Check comments for decisions
        for comment in &ticket.comments {
            let comment_lower = comment.to_lowercase();
            if decision_indicators
                .iter()
                .any(|ind| comment_lower.contains(ind))
            {
                decisions.push(comment.clone());
            }
        }
    }

    decisions.sort();
    decisions.dedup();
    decisions
}

// ─── HTTP helpers with retry and rate-limit handling ───

/// Shared retry wrapper for all Notion API calls.
///
/// `build_request` is called on each attempt to construct the `reqwest::RequestBuilder`.
/// This lets callers decide the HTTP method, headers, and body.
async fn notion_request_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<serde_json::Value> {
    let mut retries = 0u32;

    loop {
        let response = build_request().send().await;

        match response {
            Ok(resp) => {
                let status = resp.status();

                if status.is_success() {
                    let json: serde_json::Value = resp
                        .json()
                        .await
                        .context("Failed to parse Notion API response")?;
                    return Ok(json);
                }

                if status.as_u16() == 429 {
                    retries += 1;
                    if retries > MAX_RETRIES {
                        bail!(
                            "Notion API rate limit exceeded after {} retries",
                            MAX_RETRIES
                        );
                    }

                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(BASE_RETRY_DELAY_MS / 1000);

                    let delay = Duration::from_millis(
                        retry_after * 1000 + BASE_RETRY_DELAY_MS * retries as u64,
                    );
                    ui::warn(&format!(
                        "Rate limited. Retrying in {:?}... ({}/{})",
                        delay, retries, MAX_RETRIES
                    ));
                    tokio::time::sleep(delay).await;
                    continue;
                }

                if status.as_u16() == 401 {
                    bail!(
                        "Notion API authentication failed. Check your WIKI_NOTION_TOKEN.\n\
                         Make sure the integration has access to the database."
                    );
                }

                if status.as_u16() == 404 {
                    bail!(
                        "Notion database not found. Check the URL and make sure \
                         the integration has been shared with the database."
                    );
                }

                let error_body = resp.text().await.unwrap_or_default();
                bail!("Notion API error ({}): {}", status, error_body);
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

async fn api_request_with_retry(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let body_owned = body.cloned();
    notion_request_with_retry(|| {
        let mut req = client
            .post(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Notion-Version", NOTION_VERSION)
            .header("Content-Type", "application/json");
        if let Some(ref b) = body_owned {
            req = req.json(b);
        }
        req
    })
    .await
}

async fn api_get_with_retry(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<serde_json::Value> {
    notion_request_with_retry(|| {
        client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Notion-Version", NOTION_VERSION)
    })
    .await
}

// ─── Resume support ───

fn load_ticket_cache(path: &Path) -> Result<HashMap<String, NotionTicket>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(path).context("Failed to read ticket cache file")?;
    let tickets: Vec<NotionTicket> =
        serde_json::from_str(&content).context("Failed to parse ticket cache file")?;

    let map = tickets.into_iter().map(|t| (t.id.clone(), t)).collect();
    Ok(map)
}

fn save_ticket_cache(path: &Path, tickets: &[NotionTicket]) {
    // Load existing cache and merge
    let mut cached: HashMap<String, NotionTicket> = if path.exists() {
        fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str::<Vec<NotionTicket>>(&content).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect()
    } else {
        HashMap::new()
    };

    for ticket in tickets {
        cached.insert(ticket.id.clone(), ticket.clone());
    }

    let all_tickets: Vec<&NotionTicket> = cached.values().collect();
    if let Ok(json) = serde_json::to_string_pretty(&all_tickets) {
        let _ = fs::write(path, json);
    }
}

fn load_progress(path: &Path) -> Result<Option<ScanProgress>> {
    if !path.exists() {
        ui::info("No previous progress found. Starting fresh.");
        return Ok(None);
    }

    let content = fs::read_to_string(path).context("Failed to read scan progress file")?;
    let progress: ScanProgress =
        serde_json::from_str(&content).context("Failed to parse scan progress file")?;

    ui::info(&format!(
        "Resuming from {} tickets processed (started at {})",
        progress.tickets_processed, progress.started_at
    ));

    Ok(Some(progress))
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    // ─── URL parsing tests ───

    #[test]
    fn parse_notion_url_standard_format() {
        let url = "https://www.notion.so/workspace/21fbcf40a7e680d0b52de5bc49fa1121?v=abc";
        let result = parse_notion_url(url).unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_without_query_params() {
        let url = "https://www.notion.so/workspace/21fbcf40a7e680d0b52de5bc49fa1121";
        let result = parse_notion_url(url).unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_without_workspace() {
        let url = "https://notion.so/21fbcf40a7e680d0b52de5bc49fa1121?v=abc";
        let result = parse_notion_url(url).unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_bare_hex() {
        let result = parse_notion_url("21fbcf40a7e680d0b52de5bc49fa1121").unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_already_uuid() {
        let result = parse_notion_url("21fbcf40-a7e6-80d0-b52d-e5bc49fa1121").unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_with_title_prefix() {
        let url =
            "https://www.notion.so/workspace/My-Database-21fbcf40a7e680d0b52de5bc49fa1121?v=abc";
        let result = parse_notion_url(url).unwrap();
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn parse_notion_url_invalid() {
        let result = parse_notion_url("https://example.com/not-a-notion-url");
        assert!(result.is_err());
    }

    // ─── UUID formatting tests ───

    #[test]
    fn format_uuid_from_hex_works() {
        let result = format_uuid_from_hex("21fbcf40a7e680d0b52de5bc49fa1121");
        assert_eq!(result, "21fbcf40-a7e6-80d0-b52d-e5bc49fa1121");
    }

    #[test]
    fn format_uuid_from_hex_another_id() {
        let result = format_uuid_from_hex("abcdef01234567890abcdef012345678");
        assert_eq!(result, "abcdef01-2345-6789-0abc-def012345678");
    }

    // ─── Block text extraction tests ───

    #[test]
    fn extract_text_from_blocks_paragraph() {
        let blocks = vec![serde_json::json!({
            "type": "paragraph",
            "paragraph": {
                "rich_text": [
                    {"plain_text": "Hello "},
                    {"plain_text": "world"}
                ]
            }
        })];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn extract_text_from_blocks_heading() {
        let blocks = vec![serde_json::json!({
            "type": "heading_1",
            "heading_1": {
                "rich_text": [{"plain_text": "Title"}]
            }
        })];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "# Title");
    }

    #[test]
    fn extract_text_from_blocks_bullet_list() {
        let blocks = vec![
            serde_json::json!({
                "type": "bulleted_list_item",
                "bulleted_list_item": {
                    "rich_text": [{"plain_text": "Item 1"}]
                }
            }),
            serde_json::json!({
                "type": "bulleted_list_item",
                "bulleted_list_item": {
                    "rich_text": [{"plain_text": "Item 2"}]
                }
            }),
        ];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "- Item 1\n- Item 2");
    }

    #[test]
    fn extract_text_from_blocks_todo() {
        let blocks = vec![
            serde_json::json!({
                "type": "to_do",
                "to_do": {
                    "rich_text": [{"plain_text": "Done task"}],
                    "checked": true
                }
            }),
            serde_json::json!({
                "type": "to_do",
                "to_do": {
                    "rich_text": [{"plain_text": "Pending task"}],
                    "checked": false
                }
            }),
        ];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "- [x] Done task\n- [ ] Pending task");
    }

    #[test]
    fn extract_text_from_blocks_mixed() {
        let blocks = vec![
            serde_json::json!({
                "type": "heading_2",
                "heading_2": {
                    "rich_text": [{"plain_text": "Section"}]
                }
            }),
            serde_json::json!({
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{"plain_text": "Some content."}]
                }
            }),
            serde_json::json!({
                "type": "quote",
                "quote": {
                    "rich_text": [{"plain_text": "A quote"}]
                }
            }),
        ];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "## Section\nSome content.\n> A quote");
    }

    #[test]
    fn extract_text_from_blocks_skips_unknown_types() {
        let blocks = vec![
            serde_json::json!({
                "type": "image",
                "image": {"type": "external"}
            }),
            serde_json::json!({
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{"plain_text": "Text after image"}]
                }
            }),
        ];
        let result = extract_text_from_blocks(&blocks);
        assert_eq!(result, "Text after image");
    }

    // ─── Contradiction detection tests ───

    #[test]
    fn detect_contradictions_finds_opposing_tickets() {
        let tickets = vec![
            NotionTicket {
                id: "1".to_string(),
                title: "Add dark mode feature".to_string(),
                status: Some("Done".to_string()),
                date: Some("2026-01-15".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
            NotionTicket {
                id: "2".to_string(),
                title: "Remove dark mode feature".to_string(),
                status: Some("Done".to_string()),
                date: Some("2026-02-01".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
        ];

        let result = detect_contradictions(&tickets);
        assert_eq!(result.len(), 1);
        assert!(result[0].0.contains("dark mode"));
        assert!(result[0].1.contains("dark mode"));
    }

    #[test]
    fn detect_contradictions_enable_disable() {
        let tickets = vec![
            NotionTicket {
                id: "1".to_string(),
                title: "Enable notifications for users".to_string(),
                status: Some("Done".to_string()),
                date: Some("2026-01-01".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
            NotionTicket {
                id: "2".to_string(),
                title: "Disable notifications for users".to_string(),
                status: Some("In Progress".to_string()),
                date: Some("2026-03-01".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
        ];

        let result = detect_contradictions(&tickets);
        assert!(!result.is_empty());
    }

    #[test]
    fn detect_contradictions_no_false_positive() {
        let tickets = vec![
            NotionTicket {
                id: "1".to_string(),
                title: "Add payment processing".to_string(),
                status: Some("Done".to_string()),
                date: Some("2026-01-01".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
            NotionTicket {
                id: "2".to_string(),
                title: "Add user authentication".to_string(),
                status: Some("Done".to_string()),
                date: Some("2026-02-01".to_string()),
                tags: vec![],
                description: String::new(),
                comments: vec![],
            },
        ];

        let result = detect_contradictions(&tickets);
        assert!(result.is_empty());
    }

    // ─── Domain mapping tests ───

    #[test]
    fn map_ticket_to_domain_by_tag() {
        let ticket = NotionTicket {
            id: "1".to_string(),
            title: "Fix invoice bug".to_string(),
            status: None,
            date: None,
            tags: vec!["billing".to_string()],
            description: String::new(),
            comments: vec![],
        };

        let domains = vec!["billing".to_string(), "auth".to_string()];
        let result = map_ticket_to_domain(&ticket, &domains);
        assert_eq!(result, "billing");
    }

    #[test]
    fn map_ticket_to_domain_by_title_keyword() {
        let ticket = NotionTicket {
            id: "1".to_string(),
            title: "Update auth flow for SSO".to_string(),
            status: None,
            date: None,
            tags: vec![],
            description: String::new(),
            comments: vec![],
        };

        let domains = vec!["billing".to_string(), "auth".to_string()];
        let result = map_ticket_to_domain(&ticket, &domains);
        assert_eq!(result, "auth");
    }

    #[test]
    fn map_ticket_to_domain_fallback_to_tag() {
        let ticket = NotionTicket {
            id: "1".to_string(),
            title: "Something unrelated".to_string(),
            status: None,
            date: None,
            tags: vec!["Infrastructure".to_string()],
            description: String::new(),
            comments: vec![],
        };

        let domains = vec!["billing".to_string(), "auth".to_string()];
        let result = map_ticket_to_domain(&ticket, &domains);
        assert_eq!(result, "infrastructure");
    }

    #[test]
    fn map_ticket_to_domain_uncategorized() {
        let ticket = NotionTicket {
            id: "1".to_string(),
            title: "Something".to_string(),
            status: None,
            date: None,
            tags: vec![],
            description: String::new(),
            comments: vec![],
        };

        let domains = vec!["billing".to_string()];
        let result = map_ticket_to_domain(&ticket, &domains);
        assert_eq!(result, "uncategorized");
    }

    // ─── ScanProgress serialization tests ───

    #[test]
    fn scan_progress_serialization_roundtrip() {
        let mut domains_mapped = HashMap::new();
        domains_mapped.insert(
            "billing".to_string(),
            vec!["ticket-1".to_string(), "ticket-2".to_string()],
        );

        let progress = ScanProgress {
            notion_url: "https://notion.so/test".to_string(),
            total_tickets: 42,
            last_cursor: Some("cursor-abc".to_string()),
            tickets_processed: 20,
            batch_size: 100,
            domains_mapped,
            started_at: "2026-03-26T10:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&progress).unwrap();
        let parsed: ScanProgress = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.notion_url, "https://notion.so/test");
        assert_eq!(parsed.total_tickets, 42);
        assert_eq!(parsed.last_cursor, Some("cursor-abc".to_string()));
        assert_eq!(parsed.tickets_processed, 20);
        assert_eq!(parsed.batch_size, 100);
        assert_eq!(parsed.domains_mapped.len(), 1);
        assert_eq!(parsed.started_at, "2026-03-26T10:00:00Z");
    }

    #[test]
    fn scan_progress_without_cursor() {
        let progress = ScanProgress {
            notion_url: "test".to_string(),
            total_tickets: 0,
            last_cursor: None,
            tickets_processed: 0,
            batch_size: 100,
            domains_mapped: HashMap::new(),
            started_at: "2026-03-26T10:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&progress).unwrap();
        let parsed: ScanProgress = serde_json::from_str(&json).unwrap();
        assert!(parsed.last_cursor.is_none());
    }

    // ─── Token resolution tests ───

    #[test]
    fn resolve_token_from_env_var() {
        // This test just checks the env var path
        // SAFETY: This test runs in isolation; no other thread reads this env var concurrently.
        unsafe { std::env::set_var("WIKI_NOTION_TOKEN", "test-token-123") };
        let result = resolve_token(Path::new("/tmp/nonexistent"));
        unsafe { std::env::remove_var("WIKI_NOTION_TOKEN") };
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-token-123");
    }

    #[test]
    fn read_token_from_env_file_works() {
        let dir = tempfile::TempDir::new().unwrap();
        let wiki_dir = dir.path();
        fs::write(
            wiki_dir.join(".env"),
            "WIKI_CLIENT_NAME=test\nWIKI_NOTION_TOKEN=secret-token-456\nWIKI_OTHER=foo\n",
        )
        .unwrap();

        let token = read_token_from_env_file(wiki_dir);
        assert_eq!(token, Some("secret-token-456".to_string()));
    }

    #[test]
    fn read_token_from_env_file_empty_value() {
        let dir = tempfile::TempDir::new().unwrap();
        let wiki_dir = dir.path();
        fs::write(wiki_dir.join(".env"), "WIKI_NOTION_TOKEN=\n").unwrap();

        let token = read_token_from_env_file(wiki_dir);
        assert!(token.is_none());
    }

    #[test]
    fn read_token_from_env_file_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let token = read_token_from_env_file(dir.path());
        assert!(token.is_none());
    }

    #[test]
    fn read_token_from_env_file_missing_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let wiki_dir = dir.path();
        fs::write(wiki_dir.join(".env"), "WIKI_CLIENT_NAME=test\n").unwrap();

        let token = read_token_from_env_file(wiki_dir);
        assert!(token.is_none());
    }
}
