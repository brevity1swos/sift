//! Project-level config stored at `.sift/config.toml`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Loose,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_ignore_globs")]
    pub ignore_globs: Vec<String>,
}

fn default_ignore_globs() -> Vec<String> {
    vec!["**/.git/**".into(), "**/target/**".into(), "**/node_modules/**".into()]
}

impl Default for Config {
    fn default() -> Self {
        Self { mode: Mode::default(), ignore_globs: default_ignore_globs() }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_mode_is_loose() {
        let c = Config::default();
        assert_eq!(c.mode, Mode::Loose);
    }

    #[test]
    fn load_returns_default_if_missing() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("config.toml");
        let c = Config::load(&p).unwrap();
        assert_eq!(c.mode, Mode::Loose);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("nested").join("config.toml");
        let c = Config { mode: Mode::Strict, ..Config::default() };
        c.save(&p).unwrap();
        let back = Config::load(&p).unwrap();
        assert_eq!(back.mode, Mode::Strict);
        assert_eq!(back.ignore_globs, c.ignore_globs);
    }
}
