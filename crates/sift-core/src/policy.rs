//! Policy-gated writes: per-path rules for auto-allow, require-review, or deny.
//!
//! Policy is defined in `.sift/policy.yml` with rules like:
//!
//! ```yaml
//! rules:
//!   - path: "src/**"
//!     action: allow
//!   - path: "*.sql"
//!     action: review
//!   - path: ".env*"
//!     action: deny
//! ```
//!
//! Actions:
//!   - `allow`: pre-tool hook exits 0 (no gate)
//!   - `review`: pre-tool hook exits 0 but prints a note to stderr
//!   - `deny`: pre-tool hook exits 2 (blocks the tool call)
//!
//! Rules are evaluated top-to-bottom; first match wins. If no rule matches,
//! the default action is `allow`.

use anyhow::{Context, Result};
use globset::{Glob, GlobMatcher};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Review,
    Deny,
}

#[derive(Debug, Deserialize)]
struct RuleRaw {
    path: String,
    action: Action,
}

#[derive(Debug)]
pub struct Rule {
    pub matcher: GlobMatcher,
    pub pattern: String,
    pub action: Action,
}

#[derive(Debug)]
pub struct Policy {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    rules: Vec<RuleRaw>,
}

impl Policy {
    /// Load policy from `.sift/policy.yml`. Returns an empty policy (all-allow)
    /// if the file doesn't exist.
    pub fn load(policy_path: &Path) -> Result<Self> {
        if !policy_path.exists() {
            return Ok(Self { rules: vec![] });
        }
        let text = fs::read_to_string(policy_path)
            .with_context(|| format!("reading policy {}", policy_path.display()))?;
        let raw: PolicyFile = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("parsing policy {}", policy_path.display()))?;

        let mut rules = Vec::with_capacity(raw.rules.len());
        for r in raw.rules {
            let glob = Glob::new(&r.path)
                .with_context(|| format!("invalid glob pattern '{}' in policy", r.path))?;
            rules.push(Rule {
                matcher: glob.compile_matcher(),
                pattern: r.path,
                action: r.action,
            });
        }
        Ok(Self { rules })
    }

    /// Evaluate a relative file path against the policy. Returns the action
    /// for the first matching rule, or `Action::Allow` if no rule matches.
    pub fn evaluate(&self, rel_path: &Path) -> Action {
        for rule in &self.rules {
            if rule.matcher.is_match(rel_path) {
                return rule.action;
            }
        }
        Action::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_policy(td: &TempDir, content: &str) -> PathBuf {
        let path = td.path().join("policy.yml");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn empty_policy_allows_everything() {
        let p = Policy { rules: vec![] };
        assert_eq!(p.evaluate(Path::new("anything.rs")), Action::Allow);
    }

    #[test]
    fn missing_file_returns_empty_policy() {
        let td = TempDir::new().unwrap();
        let p = Policy::load(&td.path().join("nonexistent.yml")).unwrap();
        assert!(p.rules.is_empty());
    }

    #[test]
    fn deny_env_files() {
        let td = TempDir::new().unwrap();
        let path = write_policy(
            &td,
            "rules:\n  - path: \".env*\"\n    action: deny\n",
        );
        let p = Policy::load(&path).unwrap();
        assert_eq!(p.evaluate(Path::new(".env")), Action::Deny);
        assert_eq!(p.evaluate(Path::new(".env.local")), Action::Deny);
        assert_eq!(p.evaluate(Path::new("src/main.rs")), Action::Allow);
    }

    #[test]
    fn first_match_wins() {
        let td = TempDir::new().unwrap();
        let path = write_policy(
            &td,
            "rules:\n  - path: \"src/**\"\n    action: allow\n  - path: \"**/*.rs\"\n    action: deny\n",
        );
        let p = Policy::load(&path).unwrap();
        // src/main.rs matches "src/**" first → allow
        assert_eq!(p.evaluate(Path::new("src/main.rs")), Action::Allow);
        // lib.rs matches "**/*.rs" → deny
        assert_eq!(p.evaluate(Path::new("lib.rs")), Action::Deny);
    }

    #[test]
    fn review_action() {
        let td = TempDir::new().unwrap();
        let path = write_policy(
            &td,
            "rules:\n  - path: \"*.sql\"\n    action: review\n",
        );
        let p = Policy::load(&path).unwrap();
        assert_eq!(p.evaluate(Path::new("schema.sql")), Action::Review);
        assert_eq!(p.evaluate(Path::new("src/foo.rs")), Action::Allow);
    }
}
