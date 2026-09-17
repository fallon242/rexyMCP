//! Literal terms that must never reach a cloud model — short site codes, system
//! acronyms, and product names that are ordinary words, the artifacts a NER
//! detector cannot find. Each alias is replaced with the entry's code
//! (`[SITE_1]`), next to the NER pre-scan's `[REDACTED:…]` markers.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// One term entry as stored in the term file.
#[derive(Debug, Deserialize)]
struct TermsFile {
    entries: Vec<TermEntry>,
}

/// One entry in the file: a code and the aliases that map to it. `embed` lets an
/// alias match at the start of a longer identifier; `match` (any value) forces a
/// case-sensitive match.
#[derive(Debug, Deserialize)]
struct TermEntry {
    code: String,
    aliases: Vec<String>,
    #[serde(default)]
    embed: bool,
    #[serde(default, rename = "match")]
    case_sensitive: Option<String>,
}

/// A compiled term file: one regex whose capture groups are the aliases, longest
/// first, and a group-index → code map.
#[derive(Debug, Default)]
pub struct LiteralTerms {
    regex: Option<regex::Regex>,
    codes: Vec<String>,
}

impl LiteralTerms {
    /// Load and compile a term file. Any failure is `Error::Privacy` naming the
    /// path — a term file that silently fails to load is worse than having none,
    /// because the operator believes they are protected.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| Error::Privacy(format!("terms_file {}: {}", path.display(), e)))?;
        let parsed: TermsFile = serde_json::from_str(&raw).map_err(|e| {
            Error::Privacy(format!(
                "terms_file {}: malformed JSON: {}",
                path.display(),
                e
            ))
        })?;
        Self::from_entries(&parsed.entries, path)
    }

    fn from_entries(entries: &[TermEntry], path: &Path) -> Result<Self> {
        let mut bodies: Vec<String> = Vec::new();
        let mut flags: Vec<bool> = Vec::new();
        let mut codes: Vec<String> = Vec::new();
        for entry in entries {
            if entry.code.is_empty() {
                return Err(Error::Privacy(format!(
                    "terms_file {}: an entry is missing its `code`",
                    path.display()
                )));
            }
            if entry.aliases.iter().any(|a| a.is_empty()) {
                return Err(Error::Privacy(format!(
                    "terms_file {}: entry `{}` has an empty alias",
                    path.display(),
                    entry.code
                )));
            }
            if entry.aliases.is_empty() {
                return Err(Error::Privacy(format!(
                    "terms_file {}: entry `{}` has an empty `aliases` list",
                    path.display(),
                    entry.code
                )));
            }
            for alias in &entry.aliases {
                if alias.trim().is_empty() {
                    return Err(Error::Privacy(format!(
                        "terms_file {}: entry `{}` has an empty alias",
                        path.display(),
                        entry.code
                    )));
                }
                let body = alias
                    .split_whitespace()
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join(r"\s+");
                let first_is_word = alias
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                let last_is_word = alias
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                let left = if first_is_word { r"\b" } else { "" };
                let right = if last_is_word && !entry.embed {
                    r"\b"
                } else {
                    ""
                };
                bodies.push(format!("{left}{body}{right}"));
                flags.push(entry.case_sensitive.is_none());
                codes.push(entry.code.clone());
            }
        }
        // Longest alias first: Rust's regex uses leftmost-first alternation, so at
        // a given start position the longest alias wins. The case flag rides in
        // the group so a case-sensitive entry (`match` present) does not inherit
        // a global `(?i)`.
        let mut order: Vec<usize> = (0..bodies.len()).collect();
        order.sort_by(|&a, &b| bodies[b].len().cmp(&bodies[a].len()));
        let pattern = order
            .iter()
            .map(|&i| {
                let f = if flags[i] { "(?i)" } else { "" };
                format!("({f}{})", bodies[i])
            })
            .collect::<Vec<_>>()
            .join("|");
        let regex = regex::Regex::new(&pattern).map_err(|e| {
            Error::Privacy(format!("terms_file {}: bad pattern: {}", path.display(), e))
        })?;
        let mut codes_by_group = vec![String::new(); order.len()];
        for (g, &i) in order.iter().enumerate() {
            codes_by_group[g] = codes[i].clone();
        }
        Ok(Self {
            regex: Some(regex),
            codes: codes_by_group,
        })
    }

    /// Replace every listed alias in `text` with `[CODE]`. One `\n` is re-emitted
    /// per `\n` inside a matched span, so the line count is preserved.
    /// `LiteralTerms::default()` returns `text` unchanged.
    pub fn mask(&self, text: &str) -> String {
        let Some(regex) = &self.regex else {
            return text.to_string();
        };
        let mut out = String::new();
        let mut last = 0;
        for caps in regex.captures_iter(text) {
            let Some(m) = caps.get(0) else {
                continue;
            };
            out.push_str(&text[last..m.start()]);
            let (code, _body) = self.matched(&caps);
            let matched = &text[m.start()..m.end()];
            out.push_str(&format!("[{code}]"));
            out.push_str(&"\n".repeat(matched.matches('\n').count()));
            last = m.end();
        }
        out.push_str(&text[last..]);
        out
    }

    /// Find the capture group that matched in `caps`, returning (code, matched text).
    fn matched<'t>(&self, caps: &'t regex::Captures<'t>) -> (String, &'t str) {
        for (g, code) in self.codes.iter().enumerate() {
            if let Some(cap) = caps.get(g + 1) {
                return (code.clone(), cap.as_str());
            }
        }
        unreachable!("regex always has exactly one matching group")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_terms(dir: &Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("terms.json");
        std::fs::write(&p, body).unwrap();
        p
    }

    const ALL: &str = r#"
        { "entries": [
          { "code": "SITE_1", "aliases": ["Plant Nine", "PLN"] },
          { "code": "SYS_1",  "aliases": ["Quillsys"], "embed": true },
          { "code": "SYS_2",  "aliases": ["Ledgers"],  "match": "case-sensitive" }
        ] }
    "#;

    #[test]
    fn loads_entries_and_flags() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(dir.path(), ALL);
        let terms = LiteralTerms::load(&p).unwrap();
        assert_eq!(terms.mask("go to PLN now"), "go to [SITE_1] now");
        assert_eq!(terms.mask("QuillsysExport"), "[SYS_1]Export");
        assert_eq!(
            terms.mask("the Ledgers and ledgers"),
            "the [SYS_2] and ledgers"
        );
        // Flags default off: `Quillsys` alone is not embed (only SYS_1 is embed),
        // so the default entry for `PLN` needs both boundaries.
        assert_eq!(terms.mask("PLNX"), "PLNX");
        assert_eq!(terms.mask("XPLN"), "XPLN");
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.json");
        let err = LiteralTerms::load(&p).unwrap_err();
        let Error::Privacy(m) = err else {
            panic!("expected Privacy");
        };
        assert!(m.contains("nope.json"), "{m}");
    }

    #[test]
    fn malformed_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(dir.path(), "{ not json");
        let err = LiteralTerms::load(&p).unwrap_err();
        let Error::Privacy(m) = err else {
            panic!("expected Privacy");
        };
        assert!(m.contains("terms.json"), "{m}");
    }

    #[test]
    fn entry_without_aliases_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(dir.path(), r#"{"entries": [{"code": "A"}]}"#);
        let err = LiteralTerms::load(&p).unwrap_err();
        let Error::Privacy(m) = err else {
            panic!("expected Privacy");
        };
        assert!(m.contains("terms.json"), "{m}");
    }

    #[test]
    fn empty_alias_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [{"code": "A", "aliases": ["  "]}]}"#,
        );
        let err = LiteralTerms::load(&p).unwrap_err();
        let Error::Privacy(m) = err else {
            panic!("expected Privacy");
        };
        assert!(m.contains("terms.json"), "{m}");
    }

    #[test]
    fn default_entry_requires_both_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [{"code": "SITE_1", "aliases": ["PLN"]}]}"#,
        );
        let terms = LiteralTerms::load(&p).unwrap();
        assert_eq!(terms.mask("go to PLN now"), "go to [SITE_1] now");
        assert_eq!(terms.mask("PLNX"), "PLNX");
        assert_eq!(terms.mask("XPLN"), "XPLN");
    }

    #[test]
    fn embed_entry_matches_inside_an_identifier() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [{"code": "SYS_1", "aliases": ["Quillsys"], "embed": true}]}"#,
        );
        let terms = LiteralTerms::load(&p).unwrap();
        assert_eq!(terms.mask("QuillsysExport"), "[SYS_1]Export");
        assert_eq!(terms.mask("myQuillsys"), "myQuillsys");
    }

    #[test]
    fn case_sensitive_entry_spares_the_common_noun() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [{"code": "SYS_2", "aliases": ["Ledgers"], "match": "case-sensitive"}]}"#,
        );
        let terms = LiteralTerms::load(&p).unwrap();
        assert_eq!(terms.mask("the Ledgers"), "the [SYS_2]");
        assert_eq!(terms.mask("the ledgers"), "the ledgers");
    }

    #[test]
    fn wrapped_alias_matches_and_keeps_the_break() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [{"code": "SITE_1", "aliases": ["Plant Nine"]}]}"#,
        );
        let terms = LiteralTerms::load(&p).unwrap();
        let masked = terms.mask("at Plant\nNine today");
        assert_eq!(masked, "at [SITE_1]\n today");
        assert_eq!(
            masked.matches('\n').count(),
            "at Plant\nNine today".matches('\n').count()
        );
    }

    #[test]
    fn longest_alias_wins() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_terms(
            dir.path(),
            r#"{"entries": [
              {"code": "B", "aliases": ["Plant Nine"]},
              {"code": "A", "aliases": ["Plant"]}
            ]}"#,
        );
        let terms = LiteralTerms::load(&p).unwrap();
        assert_eq!(terms.mask("Plant Nine"), "[B]");
    }

    #[test]
    fn default_terms_leave_text_unchanged() {
        let terms = LiteralTerms::default();
        for s in ["hello", "Plant Nine", "PLN"] {
            assert_eq!(terms.mask(s), s);
        }
    }
}
