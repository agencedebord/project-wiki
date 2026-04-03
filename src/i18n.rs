/// Internationalization support for wiki content generation.
///
/// Only the **generated wiki content** is translated (section headers, static text in markdown).
/// CLI output messages (terminal UI) remain in English.

const SUPPORTED_LANGUAGES: &[&str] = &["en", "fr"];

/// Returns true if the language code is explicitly supported.
pub fn is_supported(lang: &str) -> bool {
    SUPPORTED_LANGUAGES.contains(&lang)
}

/// Returns the language name suitable for LLM prompt instructions.
pub fn language_name(lang: &str) -> &str {
    match lang {
        "fr" => "French",
        "en" => "English",
        _ => "English",
    }
}

/// Translate a wiki section key into the appropriate language.
pub fn t<'a>(key: &'a str, lang: &str) -> &'a str {
    match (key, lang) {
        // ─── Domain overview sections ───
        ("what_this_domain_does", "fr") => "Ce que fait ce domaine",
        ("what_this_domain_does", _) => "What this domain does",

        ("key_behaviors", "fr") => "Comportements clés",
        ("key_behaviors", _) => "Key behaviors",

        ("domain_interactions", "fr") => "Interactions avec d'autres domaines",
        ("domain_interactions", _) => "Domain interactions",

        ("gotchas", "fr") => "Pièges et cas limites",
        ("gotchas", _) => "Gotchas and edge cases",

        ("notes_from_code", "fr") => "Notes du code",
        ("notes_from_code", _) => "Notes from code",

        ("dependencies", "fr") => "Dépendances",
        ("dependencies", _) => "Dependencies",

        ("referenced_by", "fr") => "Référencé par",
        ("referenced_by", _) => "Referenced by",

        ("description", "fr") => "Description",
        ("description", _) => "Description",

        ("llm_not_available", "fr") => "L'analyse LLM n'était pas disponible pour ce domaine.",
        ("llm_not_available", _) => "LLM analysis was not available for this domain.",

        ("detected_models", "fr") => "Modèles détectés",
        ("detected_models", _) => "Detected models",

        ("detected_routes", "fr") => "Routes détectées",
        ("detected_routes", _) => "Detected routes",

        // ─── Index ───
        ("domains", "fr") => "Domaines",
        ("domains", _) => "Domains",

        ("no_domains_yet", "fr") => "_Aucun domaine documenté._",
        ("no_domains_yet", _) => "_No domains documented yet._",

        ("recent_decisions", "fr") => "Décisions récentes",
        ("recent_decisions", _) => "Recent decisions",

        ("last_updated", "fr") => "Dernière mise à jour",
        ("last_updated", _) => "Last updated",

        ("initialized_on", "fr") => "Initialisé le",
        ("initialized_on", _) => "Initialized on",

        ("auto_generated_kb", "fr") => "Base de connaissances auto-générée. Gérée par",
        ("auto_generated_kb", _) => "Auto-generated knowledge base. Managed by",

        // ─── Graph ───
        ("dependency_graph", "fr") => "Graphe de dépendances des domaines",
        ("dependency_graph", _) => "Domain dependency graph",

        ("auto_generated_scan", "fr") => "Auto-généré depuis le scan du code. Ne pas éditer manuellement.",
        ("auto_generated_scan", _) => "Auto-generated from codebase scan. Do not edit manually.",

        // ─── Needs review ───
        ("needs_review", "fr") => "À vérifier",
        ("needs_review", _) => "Needs review",

        ("needs_review_intro", "fr") => "Les éléments ci-dessous ont été générés automatiquement et nécessitent une validation humaine.\n> Répondez ou validez chaque élément, puis supprimez-le de cette liste.",
        ("needs_review_intro", _) => "Items below were generated automatically and need human validation.\n> Answer or validate each item, then remove it from this list.",

        ("open_questions", "fr") => "Questions ouvertes",
        ("open_questions", _) => "Open questions",

        ("unresolved_contradictions", "fr") => "Contradictions non résolues",
        ("unresolved_contradictions", _) => "Unresolved contradictions",

        ("no_open_questions", "fr") => "_Aucune question ouverte trouvée._",
        ("no_open_questions", _) => "_No open questions found._",

        ("none_detected", "fr") => "_Aucune détectée._",
        ("none_detected", _) => "_None detected._",

        // ─── Index (wiki::index::run) ───
        ("auto_generated_index", "fr") => "Auto-généré. Ne pas éditer manuellement.",
        ("auto_generated_index", _) => "Auto-generated. Do not edit manually.",

        ("no_decisions_yet", "fr") => "_Aucune décision enregistrée._",
        ("no_decisions_yet", _) => "_No decisions recorded yet._",

        ("decisions", "fr") => "Décisions",
        ("decisions", _) => "Decisions",

        ("health", "fr") => "Santé",
        ("health", _) => "Health",

        // ─── Graph (wiki::graph::run) ───
        ("auto_generated_graph", "fr") => "Auto-généré depuis les notes de domaine. Ne pas éditer manuellement.",
        ("auto_generated_graph", _) => "Auto-generated from domain notes. Do not edit manually.",

        // ─── Domain overview template (add domain) ───
        ("description_placeholder", "fr") => "_Brève description de ce domaine._",
        ("description_placeholder", _) => "_Brief description of this domain._",

        ("key_behaviors_placeholder", "fr") => "_Documentez les comportements clés ici. Taguez chacun avec un niveau de confiance._",
        ("key_behaviors_placeholder", _) => "_Document key behaviors here. Tag each with a confidence level._",

        ("business_rules", "fr") => "Règles métier",
        ("business_rules", _) => "Business rules",

        ("business_rules_placeholder", "fr") => "_Documentez les règles métier spécifiques à ce domaine._",
        ("business_rules_placeholder", _) => "_Document business rules specific to this domain._",

        ("dependencies_placeholder", "fr") => "_Listez les domaines dont celui-ci dépend._",
        ("dependencies_placeholder", _) => "_List domains this one depends on._",

        ("referenced_by_placeholder", "fr") => "_Listez les domaines qui dépendent de celui-ci._",
        ("referenced_by_placeholder", _) => "_List domains that depend on this one._",

        // ─── Notion import ───
        ("business_rules_from_notion", "fr") => "Règles métier (depuis Notion)",
        ("business_rules_from_notion", _) => "Business rules (from Notion)",

        ("decisions_from_notion", "fr") => "Décisions (depuis Notion)",
        ("decisions_from_notion", _) => "Decisions (from Notion)",

        ("notion_tickets", "fr") => "Tickets Notion",
        ("notion_tickets", _) => "Notion tickets",

        ("contradictions_from_notion", "fr") => "Contradictions (depuis Notion)",
        ("contradictions_from_notion", _) => "Contradictions (from Notion)",

        ("contradictions_intro", "fr") => "Ces paires de tickets peuvent contenir des informations contradictoires. Le ticket le plus récent prévaut probablement sur l'ancien.",
        ("contradictions_intro", _) => "These ticket pairs may contain contradictory information. The newer ticket likely supersedes the older one.",

        // ─── Candidates ───
        ("memory_candidates", "fr") => "Candidats mémoire",
        ("memory_candidates", _) => "Memory Candidates",

        ("candidates_intro", "fr") => "Propositions auto-générées à confirmer, rejeter ou reformuler.\n> Ces candidats ne sont pas encore de la mémoire confirmée.\n> Éditez ce fichier ou utilisez `codefidence promote <id>` pour valider.",
        ("candidates_intro", _) => "Auto-generated proposals to confirm, reject, or reformulate.\n> These candidates are not yet confirmed memory.\n> Edit this file or use `codefidence promote <id>` to validate.",

        ("no_candidates", "fr") => "Aucun candidat détecté.",
        ("no_candidates", _) => "No candidates detected.",

        // ─── Fallback ───
        (_, _) => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_default() {
        assert_eq!(t("key_behaviors", "en"), "Key behaviors");
        assert_eq!(t("key_behaviors", "xx"), "Key behaviors");
    }

    #[test]
    fn french_translations() {
        assert_eq!(t("key_behaviors", "fr"), "Comportements clés");
        assert_eq!(t("gotchas", "fr"), "Pièges et cas limites");
        assert_eq!(t("domains", "fr"), "Domaines");
    }

    #[test]
    fn unknown_key_returns_key() {
        assert_eq!(t("unknown_key_xyz", "en"), "unknown_key_xyz");
        assert_eq!(t("unknown_key_xyz", "fr"), "unknown_key_xyz");
    }

    #[test]
    fn language_name_mapping() {
        assert_eq!(language_name("fr"), "French");
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("xx"), "English");
    }
}
