use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct WikiConfig {
    pub staleness_days: u32,
    pub auto_index: bool,
}

impl Default for WikiConfig {
    fn default() -> Self {
        Self {
            staleness_days: 30,
            auto_index: true,
        }
    }
}

pub fn load(wiki_dir: &Path) -> WikiConfig {
    let config_path = wiki_dir.join("config.toml");
    let mut config = WikiConfig::default();

    if let Ok(content) = fs::read_to_string(&config_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');
                match key {
                    "staleness_days" => {
                        if let Ok(days) = value.parse() {
                            config.staleness_days = days;
                        }
                    }
                    "auto_index" => {
                        config.auto_index = value == "true";
                    }
                    _ => {} // ignore unknown keys
                }
            }
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    #[test]
    fn default_config() {
        let config = WikiConfig::default();
        assert_eq!(config.staleness_days, 30);
        assert!(config.auto_index);
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let dir = TempDir::new().unwrap();
        let config = load(dir.path());
        assert_eq!(config.staleness_days, 30);
        assert!(config.auto_index);
    }

    #[test]
    fn load_config_from_file() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        let mut f = fs::File::create(&config_path).unwrap();
        writeln!(f, "# project-wiki configuration").unwrap();
        writeln!(f, "staleness_days = 60").unwrap();
        writeln!(f, "auto_index = false").unwrap();

        let config = load(dir.path());
        assert_eq!(config.staleness_days, 60);
        assert!(!config.auto_index);
    }

    #[test]
    fn load_config_ignores_comments_and_unknown_keys() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        let mut f = fs::File::create(&config_path).unwrap();
        writeln!(f, "# comment").unwrap();
        writeln!(f, "staleness_days = 14").unwrap();
        writeln!(f, "unknown_key = hello").unwrap();
        writeln!(f).unwrap();

        let config = load(dir.path());
        assert_eq!(config.staleness_days, 14);
        assert!(config.auto_index); // default since not set
    }
}
