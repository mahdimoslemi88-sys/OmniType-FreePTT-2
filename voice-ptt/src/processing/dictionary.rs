//! Technical-term dictionary using Aho-Corasick multi-pattern matching.
//!
//! ASR engines mangle technical terms in Persian speech ("پایتون" →
//! "پاتون", "جاوااسکریپت" → "جاوا اسکریپ"). The dictionary maps the common
//! mis-transcriptions back to the canonical form. Matching is done with
//! Aho-Corasick so hundreds of patterns cost a single pass over the text.

use aho_corasick::AhoCorasick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One correction rule: replace `from` (in normalized text) with `to`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub from: String,
    pub to: String,
}

/// Default seed corrections (extendable via `dictionary.toml`).
fn default_corrections() -> Vec<Correction> {
    const SEED: &[(&str, &str)] = &[
        ("پاتون", "پایتون"),
        ("پايتون", "پایتون"),
        ("جاوا اسکریپت", "جاوااسکریپت"),
        ("جاوا سکریپت", "جاوااسکریپت"),
        ("نود جی اس", "Node.js"),
        ("ری اکت", "React"),
        ("ریکت", "React"),
        ("داکر", "Docker"),
        ("کوبرنتیز", "Kubernetes"),
        ("دیتابیس", "دیتابیس"),
        ("داده بیس", "دیتابیس"),
        ("گیتهاب", "گیت‌هاب"),
        ("گیت هاب", "گیت‌هاب"),
        ("لینوکس", "لینوکس"),
        ("ترمینال", "ترمینال"),
        ("سرور", "سرور"),
        ("اپی آی", "API"),
        ("ای پی آی", "API"),
        ("اچ تی ام ال", "HTML"),
        ("سی اس اس", "CSS"),
        ("جی اس او ان", "JSON"),
        ("جیسون", "JSON"),
        ("پای تچ", "پچ"),
        ("کامپایلر", "کامپایلر"),
        ("کامپایلر", "کامپایلر"),
        ("راست", "Rust"),
        ("گیت هاب", "گیت‌هاب"),
        ("وی اس کد", "VS Code"),
        ("ویژوال استودیو", "Visual Studio"),
    ];
    SEED.iter()
        .filter(|(a, b)| a != b)
        .map(|(from, to)| Correction {
            from: from.to_string(),
            to: to.to_string(),
        })
        .collect()
}

/// Multi-pattern dictionary with longest-match-wins replacement.
pub struct Dictionary {
    matcher: AhoCorasick,
    replacements: Vec<String>,
}

impl Dictionary {
    /// Builds a dictionary from correction rules.
    pub fn new(corrections: Vec<Correction>) -> Self {
        // Deduplicate by `from`, keeping the last definition.
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for c in corrections {
            if !c.from.is_empty() && c.from != c.to {
                map.insert(c.from, c.to);
            }
        }
        let patterns: Vec<String> = map.keys().cloned().collect();
        let replacements = map.into_values().collect();

        let matcher = AhoCorasick::builder()
            .match_kind(aho_corasick::MatchKind::LeftmostLongest)
            .build(&patterns)
            .expect("aho-corasick build cannot fail with non-empty patterns");

        Self {
            matcher,
            replacements,
        }
    }

    /// The default seed dictionary.
    pub fn with_defaults() -> Self {
        Self::new(default_corrections())
    }

    /// Loads user corrections from a TOML file and merges them with defaults.
    /// File format:
    /// ```toml
    /// [[corrections]]
    /// from = "پاتون"
    /// to   = "پایتون"
    /// ```
    pub fn load_or_default(path: &Path) -> Self {
        let mut corrections = default_corrections();
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<DictionaryFile>(&content) {
                Ok(file) => corrections.extend(file.corrections),
                Err(e) => tracing::warn!(%e, path = %path.display(), "invalid dictionary file"),
            },
            Err(_) => {
                // No user dictionary — fine, use defaults.
            }
        }
        Self::new(corrections)
    }

    /// Applies all corrections. Replacements are plain text (no capture
    /// groups), so a single ordered pass with leftmost-longest semantics is
    /// sufficient.
    pub fn correct(&self, text: &str) -> String {
        if self.replacements.is_empty() || text.is_empty() {
            return text.to_string();
        }
        self.matcher
            .replace_all(text, &self.replacements)
    }

    /// Number of active rules.
    pub fn len(&self) -> usize {
        self.replacements.len()
    }

    /// Whether the dictionary has no rules.
    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct DictionaryFile {
    #[serde(default)]
    corrections: Vec<Correction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrects_python_mishearing() {
        let d = Dictionary::with_defaults();
        assert_eq!(d.correct("من با پاتون کار می‌کنم"), "من با پایتون کار می‌کنم");
    }

    #[test]
    fn corrects_split_terms() {
        let d = Dictionary::with_defaults();
        assert_eq!(d.correct("جاوا اسکریپت خوب است"), "جاوااسکریپت خوب است");
    }

    #[test]
    fn leaves_normal_text_untouched() {
        let d = Dictionary::with_defaults();
        let text = "امروز هوا خیلی خوب بود";
        assert_eq!(d.correct(text), text);
    }

    #[test]
    fn multiple_replacements_in_one_pass() {
        let d = Dictionary::with_defaults();
        assert_eq!(
            d.correct("اپی آی با داکر"),
            "API با Docker"
        );
    }

    #[test]
    fn user_file_merges_with_defaults() {
        let dir = std::env::temp_dir().join("voice-ptt-dict-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dict.toml");
        std::fs::write(
            &file,
            "[[corrections]]\nfrom = \"فلان\"\nto = \"فلان‌چیز\"\n",
        )
        .unwrap();

        let d = Dictionary::load_or_default(&file);
        assert!(d.len() > 1, "user rules should merge with defaults");
        assert_eq!(d.correct("این فلان است"), "این فلان‌چیز است");
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let d = Dictionary::load_or_default(Path::new("Z:/no/such/file.toml"));
        assert!(!d.is_empty());
    }

    #[test]
    fn self_rules_are_dropped() {
        let d = Dictionary::new(vec![Correction {
            from: "سرور".into(),
            to: "سرور".into(),
        }]);
        assert!(d.is_empty(), "identity rules must be dropped");
    }
}
