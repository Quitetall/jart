//! Config + topic presets (spec §6). TOML at ~/.config/jart/config.toml.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub label: String,
    pub hf: String,
    pub pubmed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub web_port: u16,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_lamu_url")]
    pub lamu_url: String,
    #[serde(default)]
    pub topic: Vec<Topic>,
}

fn default_port() -> u16 { 8787 }
// `lamu serve` :8020 serves LOCAL models only — cloud ids (mimo-v2.5, deepseek-*,
// claude-*) return spawn_failed there. Default to a local chat model; override
// `model` in config with any id from `GET :8020/v1/models`.
fn default_model() -> String { "qwen3.6-27b-uncensored-heretic-v2-q4_k_m".into() }
fn default_lamu_url() -> String { "http://localhost:8020".into() }

impl Default for Config {
    fn default() -> Self {
        Config {
            web_port: default_port(),
            model: default_model(),
            lamu_url: default_lamu_url(),
            topic: eeg_preset(),
        }
    }
}

/// The shipped EEG/BCI preset (ported from the original artifact's TOPICS).
pub fn eeg_preset() -> Vec<Topic> {
    vec![
        Topic { id: "seizure".into(), label: "Seizure detection".into(),
            hf: "EEG seizure detection deep learning".into(),
            pubmed: "EEG seizure detection deep learning".into() },
        Topic { id: "foundation".into(), label: "Foundation models".into(),
            hf: "EEG foundation model pretraining transformer".into(),
            pubmed: "EEG foundation model machine learning".into() },
        Topic { id: "bci".into(), label: "BCI / motor imagery".into(),
            hf: "EEG motor imagery brain computer interface decoding".into(),
            pubmed: "EEG motor imagery brain computer interface".into() },
        Topic { id: "hardware".into(), label: "Hardware / AFE".into(),
            hf: "wearable EEG dry electrode hardware acquisition".into(),
            pubmed: "EEG wearable dry electrode amplifier acquisition device".into() },
    ]
}

impl Config {
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }
    /// If the user supplied no topics, fall back to the preset.
    pub fn topics(&self) -> Vec<Topic> {
        if self.topic.is_empty() { eeg_preset() } else { self.topic.clone() }
    }

    /// Load config from `explicit` if given, else `$XDG_CONFIG_HOME/jart/config.toml`
    /// (or `~/.config/jart/config.toml`). Missing file -> defaults. A present but
    /// unreadable/invalid file warns on stderr and falls back to defaults.
    pub fn load(explicit: Option<PathBuf>) -> Self {
        let path = explicit.or_else(default_config_path);
        if let Some(p) = path {
            if p.exists() {
                match std::fs::read_to_string(&p) {
                    Ok(s) => match Self::from_toml(&s) {
                        Ok(c) => return c,
                        Err(e) => eprintln!("jart: invalid config {}: {e}; using defaults", p.display()),
                    },
                    Err(e) => eprintln!("jart: cannot read {}: {e}; using defaults", p.display()),
                }
            }
        }
        Self::default()
    }
}

fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|base| base.join("jart/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_eeg_preset_and_port_8787() {
        let c = Config::default();
        assert_eq!(c.web_port, 8787);
        assert_eq!(c.model, "qwen3.6-27b-uncensored-heretic-v2-q4_k_m");
        assert_eq!(c.topics().len(), 4);
        assert_eq!(c.topics()[0].id, "seizure");
    }

    #[test]
    fn parses_user_toml_and_overrides_topics() {
        let s = r#"
            web_port = 9000
            model = "deepseek-v4-pro"
            [[topic]]
            id = "nlp"
            label = "NLP"
            hf = "language models"
            pubmed = "language models"
        "#;
        let c = Config::from_toml(s).unwrap();
        assert_eq!(c.web_port, 9000);
        assert_eq!(c.model, "deepseek-v4-pro");
        assert_eq!(c.topics().len(), 1);
        assert_eq!(c.topics()[0].id, "nlp");
    }

    #[test]
    fn load_missing_explicit_path_falls_back_to_default() {
        let c = Config::load(Some(PathBuf::from("/nonexistent/research/config.toml")));
        assert_eq!(c.web_port, 8787);
        assert_eq!(c.topics().len(), 4);
    }

    #[test]
    fn load_reads_an_explicit_toml_file() {
        let dir = std::env::temp_dir().join("research_cfg_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(&p, "web_port = 9123\nmodel = \"local-x\"\n").unwrap();
        let c = Config::load(Some(p.clone()));
        assert_eq!(c.web_port, 9123);
        assert_eq!(c.model, "local-x");
        assert_eq!(c.topics().len(), 4); // no [[topic]] -> preset
        let _ = std::fs::remove_file(p);
    }
}
