//! Repo PII pre-scan (M45) — aggregate every file's PII into a `PiiIndex`: the
//! term dictionary the outbound redactor matches against, and the set of
//! PII-bearing files the write-refuse guard blocks. Incremental via the M44
//! registry, so NER runs only on new or changed files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::PiiKind;
use super::detector::detect_deterministic;
use super::ner::NerEngine;
use super::registry::Registry;
use super::seal;
use crate::error::{Error, Result};

/// The repo's PII, stored per file so an unchanged file can reuse its entry.
/// Persisted **encrypted** (it is a PII honeypot) via [`super::seal`].
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PiiIndex {
    per_file: BTreeMap<PathBuf, Vec<(String, PiiKind)>>,
}

/// On-disk name of the sealed index inside the vault dir.
const INDEX_FILE: &str = "egress-index.enc";

impl PiiIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load the sealed index from `dir/egress-index.enc` (empty if absent).
    pub fn load(dir: &Path) -> Result<Self> {
        match fs::read(dir.join(INDEX_FILE)) {
            Ok(blob) => {
                let key = seal::load_or_create_key(dir)?;
                let plaintext = seal::unseal(&key, &blob)?;
                serde_json::from_slice(&plaintext)
                    .map_err(|e| Error::Privacy(format!("parse egress index: {e}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Seal and atomically write the index to `dir/egress-index.enc`.
    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(dir.join(".gitignore"), "*\n")?;
        let key = seal::load_or_create_key(dir)?;
        let plaintext = serde_json::to_vec(self)
            .map_err(|e| Error::Privacy(format!("serialize egress index: {e}")))?;
        let blob = seal::seal(&key, &plaintext)?;
        let tmp = dir.join("egress-index.enc.tmp");
        fs::write(&tmp, &blob)?;
        fs::rename(&tmp, dir.join(INDEX_FILE))?;
        Ok(())
    }

    /// True if `path` was found to contain any PII.
    pub fn contains_file(&self, path: &Path) -> bool {
        self.per_file.get(path).is_some_and(|pii| !pii.is_empty())
    }

    /// The PII-bearing files.
    pub fn files(&self) -> impl Iterator<Item = &PathBuf> {
        self.per_file
            .iter()
            .filter(|(_, pii)| !pii.is_empty())
            .map(|(path, _)| path)
    }

    /// Distinct PII terms to redact, **longest first** so an overlapping match
    /// prefers the longer term.
    pub fn redaction_terms(&self) -> Vec<(String, PiiKind)> {
        let mut distinct: BTreeMap<String, PiiKind> = BTreeMap::new();
        for pii in self.per_file.values() {
            for (text, kind) in pii {
                distinct.entry(text.clone()).or_insert(*kind);
            }
        }
        let mut terms: Vec<(String, PiiKind)> = distinct.into_iter().collect();
        terms.sort_by_key(|t| std::cmp::Reverse(t.0.len()));
        terms
    }

    pub fn is_empty(&self) -> bool {
        self.per_file.values().all(|pii| pii.is_empty())
    }
}

/// Scan `files` into a `PiiIndex`. A file whose content is unchanged per
/// `registry` and present in `prior` reuses its cached entry (no model call);
/// new or changed files run deterministic detection + NER. `registry` is updated
/// so a later pass can skip them.
pub async fn build_pii_index(
    files: &[(PathBuf, String)],
    ner: &NerEngine,
    registry: &mut Registry,
    prior: &PiiIndex,
) -> Result<PiiIndex> {
    let mut per_file = BTreeMap::new();
    for (path, content) in files {
        let key = path.to_string_lossy();
        if !registry.is_changed(&key, content)
            && let Some(cached) = prior.per_file.get(path)
        {
            per_file.insert(path.clone(), cached.clone());
            continue;
        }
        let mut spans = detect_deterministic(content);
        spans.extend(ner.detect(content).await?);
        let pii: Vec<(String, PiiKind)> = spans.into_iter().map(|s| (s.text, s.kind)).collect();
        registry.mark(&key, content);
        per_file.insert(path.clone(), pii);
    }
    Ok(PiiIndex { per_file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testing::MockAiClient;

    #[test]
    fn index_persists_encrypted_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        let mut idx = PiiIndex::empty();
        idx.per_file.insert(
            PathBuf::from("data.json"),
            vec![("Alice".to_string(), PiiKind::PersonName)],
        );
        idx.save(&vault).unwrap();

        let blob = std::fs::read(vault.join("egress-index.enc")).unwrap();
        assert!(
            !blob.windows(5).any(|w| w == b"Alice"),
            "the index must be encrypted at rest"
        );

        let loaded = PiiIndex::load(&vault).unwrap();
        assert!(loaded.contains_file(Path::new("data.json")));
    }

    #[test]
    fn load_missing_index_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            PiiIndex::load(&dir.path().join("absent"))
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn scans_all_files_first_pass_and_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
            r#"[]"#.to_string(),
        ]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let mut reg = Registry::load(&dir.path().join("m.json")).unwrap();
        let files = vec![
            (
                PathBuf::from("data/users.json"),
                "owner Alice email a@b.com".to_string(),
            ),
            (PathBuf::from("src/main.rs"), "fn main() {}".to_string()),
        ];

        let index = build_pii_index(&files, &ner, &mut reg, &PiiIndex::empty())
            .await
            .unwrap();

        assert!(index.contains_file(Path::new("data/users.json")));
        assert!(!index.contains_file(Path::new("src/main.rs")));
        let terms: Vec<String> = index
            .redaction_terms()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(terms.contains(&"Alice".to_string()));
        assert!(terms.contains(&"a@b.com".to_string()));
        assert_eq!(
            mock.calls().len(),
            2,
            "NER runs once per file on first pass"
        );
    }

    #[tokio::test]
    async fn unchanged_file_reuses_cache_without_ner() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
        ]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let mut reg = Registry::load(&dir.path().join("m.json")).unwrap();
        let files = vec![(PathBuf::from("data/u.json"), "Alice".to_string())];

        let first = build_pii_index(&files, &ner, &mut reg, &PiiIndex::empty())
            .await
            .unwrap();
        assert_eq!(mock.calls().len(), 1);

        let second = build_pii_index(&files, &ner, &mut reg, &first)
            .await
            .unwrap();
        assert_eq!(mock.calls().len(), 1, "unchanged file must not re-run NER");
        assert!(second.contains_file(Path::new("data/u.json")));
    }

    #[tokio::test]
    async fn changed_file_is_rescanned() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
            r#"[{"text":"Bob","type":"person_name"}]"#.to_string(),
        ]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let mut reg = Registry::load(&dir.path().join("m.json")).unwrap();

        let files_v1 = vec![(PathBuf::from("data/u.json"), "hi Alice".to_string())];
        let first = build_pii_index(&files_v1, &ner, &mut reg, &PiiIndex::empty())
            .await
            .unwrap();
        assert_eq!(mock.calls().len(), 1);

        let files_v2 = vec![(PathBuf::from("data/u.json"), "hi Bob".to_string())];
        let _second = build_pii_index(&files_v2, &ner, &mut reg, &first)
            .await
            .unwrap();
        assert_eq!(mock.calls().len(), 2, "changed content must re-run NER");
    }
}
