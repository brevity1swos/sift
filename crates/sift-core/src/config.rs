//! Project-level config stored at `.sift/config.toml`.

use anyhow::Context;
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
    vec![
        "**/.git/**".into(),
        "**/target/**".into(),
        "**/node_modules/**".into(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            ignore_globs: default_ignore_globs(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("reading config {}", path.display()))
            }
        };
        toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config parent {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        fs::write(path, text).with_context(|| format!("writing config {}", path.display()))?;
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
        let c = Config {
            mode: Mode::Strict,
            ..Config::default()
        };
        c.save(&p).unwrap();
        let back = Config::load(&p).unwrap();
        assert_eq!(back.mode, Mode::Strict);
        assert_eq!(back.ignore_globs, c.ignore_globs);
    }

    #[test]
    fn load_errors_include_file_path_on_invalid_toml() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("config.toml");
        fs::write(&p, "not valid toml ][").unwrap();
        let err = Config::load(&p).unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("config.toml"),
            "error chain should mention the file path, got: {rendered}"
        );
    }
}
