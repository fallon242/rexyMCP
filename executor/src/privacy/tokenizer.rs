//! Stable, reversible pseudonymization. Each distinct PII original maps to one
//! stable token (`Person_1`); reconstitution inverts it exactly. This is the
//! in-memory half of the "secure dictionary"; persistence and encryption at
//! rest land in phase-02.

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::detector::detect_deterministic;
use super::{PiiKind, PiiSpan};

/// A single dictionary row — the persisted, serializable unit of the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEntry {
    pub token: String,
    pub original: String,
    pub kind: PiiKind,
}

/// A reversible original↔token dictionary. The same original always interns to
/// the same token; distinct originals never collide (per-kind monotonic
/// counter).
#[derive(Debug, Default)]
pub struct TokenMap {
    forward: HashMap<String, String>,
    reverse: HashMap<String, (String, PiiKind)>,
    counters: HashMap<PiiKind, usize>,
}

impl TokenMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the stable token for `original`, minting a new one on first sight.
    pub fn intern(&mut self, original: &str, kind: PiiKind) -> String {
        if let Some(token) = self.forward.get(original) {
            return token.clone();
        }
        let n = self.counters.entry(kind).or_insert(0);
        *n += 1;
        let token = format!("{}_{}", kind.token_prefix(), n);
        self.forward.insert(original.to_string(), token.clone());
        self.reverse
            .insert(token.clone(), (original.to_string(), kind));
        token
    }

    /// Replace each span's text with its interned token. Spans are applied
    /// right-to-left so earlier byte offsets stay valid as the string shifts.
    /// Callers must pass non-overlapping spans (as `detect_deterministic`
    /// returns); overlapping ranges are a caller error.
    pub fn anonymize(&mut self, text: &str, spans: &[PiiSpan]) -> String {
        let mut ordered: Vec<&PiiSpan> = spans.iter().collect();
        ordered.sort_by_key(|s| std::cmp::Reverse(s.start));
        let mut out = text.to_string();
        for span in ordered {
            let token = self.intern(&span.text, span.kind);
            out.replace_range(span.start..span.end, &token);
        }
        out
    }

    /// Convenience: detect structured PII in `text`, then anonymize it.
    pub fn anonymize_text(&mut self, text: &str) -> String {
        let spans = detect_deterministic(text);
        self.anonymize(text, &spans)
    }

    /// Replace every known token in `text` with its original — the inverse of
    /// `anonymize` for tokens this map has interned. Word-boundary anchored so
    /// `Person_1` never matches inside `Person_12`.
    pub fn reconstitute(&self, text: &str) -> String {
        if self.reverse.is_empty() {
            return text.to_string();
        }
        let mut tokens: Vec<&String> = self.reverse.keys().collect();
        tokens.sort_by_key(|t| std::cmp::Reverse(t.len()));
        let alternation = tokens
            .iter()
            .map(|t| regex::escape(t))
            .collect::<Vec<_>>()
            .join("|");
        let re = Regex::new(&format!(r"\b(?:{alternation})\b"))
            .expect("alternation of escaped literal tokens is always a valid regex");
        re.replace_all(text, |caps: &regex::Captures| {
            match self.reverse.get(&caps[0]) {
                Some((original, _)) => original.clone(),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
    }

    /// Flatten the dictionary into serializable rows for persistence.
    pub fn entries(&self) -> Vec<VaultEntry> {
        self.reverse
            .iter()
            .map(|(token, (original, kind))| VaultEntry {
                token: token.clone(),
                original: original.clone(),
                kind: *kind,
            })
            .collect()
    }

    /// Rebuild a map from persisted rows. Each per-kind counter is restored to
    /// the max numeric suffix seen for that kind, so no future `intern` re-mints
    /// a token that collides with a persisted one.
    pub fn from_entries(entries: Vec<VaultEntry>) -> Self {
        let mut map = Self::new();
        for entry in entries {
            let n = entry
                .token
                .rsplit('_')
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let counter = map.counters.entry(entry.kind).or_insert(0);
            *counter = (*counter).max(n);
            map.forward
                .insert(entry.original.clone(), entry.token.clone());
            map.reverse
                .insert(entry.token, (entry.original, entry.kind));
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_original_gets_same_token() {
        let mut map = TokenMap::new();
        let a = map.intern("Alice", PiiKind::PersonName);
        let b = map.intern("Alice", PiiKind::PersonName);
        assert_eq!(a, b);
        assert_eq!(a, "Person_1");
    }

    #[test]
    fn distinct_originals_get_distinct_tokens() {
        let mut map = TokenMap::new();
        let a = map.intern("Alice", PiiKind::PersonName);
        let b = map.intern("Bob", PiiKind::PersonName);
        assert_ne!(a, b);
        assert_eq!(b, "Person_2");
    }

    #[test]
    fn reconstitute_inverts_anonymize() {
        let mut map = TokenMap::new();
        let original = "email a@b.com and call 555-123-4567";
        let anon = map.anonymize_text(original);
        assert!(!anon.contains("a@b.com"));
        assert_eq!(map.reconstitute(&anon), original);
    }

    #[test]
    fn anonymize_replaces_every_occurrence() {
        let mut map = TokenMap::new();
        let anon = map.anonymize_text("from a@b.com to a@b.com");
        assert_eq!(anon, "from Email_1 to Email_1");
    }

    #[test]
    fn reconstitute_leaves_token_prefix_of_longer_token_intact() {
        let mut map = TokenMap::new();
        assert_eq!(map.intern("Alice", PiiKind::PersonName), "Person_1");
        // "Person_12" is not a known token; the trailing \b stops Person_1 from
        // matching inside it.
        assert_eq!(map.reconstitute("Person_12"), "Person_12");
        assert_eq!(map.reconstitute("Person_1"), "Alice");
    }

    #[test]
    fn reconstitute_is_noop_on_empty_map() {
        let map = TokenMap::new();
        assert_eq!(map.reconstitute("Person_1 untouched"), "Person_1 untouched");
    }
}
