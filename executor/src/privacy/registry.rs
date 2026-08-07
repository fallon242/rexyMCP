//! Incremental ingestion — track each source by content hash so the gateway
//! (and its model call) re-runs only on new or changed material, while the shared
//! vault map keeps pseudonyms stable across edits.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::gateway::Gateway;
use super::tokenizer::TokenMap;
use crate::error::{Error, Result};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    hashes: BTreeMap<String, String>,
}

/// A persisted map of `source key → SHA-256 hex of its last-ingested content`.
/// Holds only opaque hashes (no PII, not reversible), so it is plaintext; the
/// reversible PII lives only in the encrypted vault.
pub struct Registry {
    path: PathBuf,
    manifest: Manifest,
}

impl Registry {
    /// Load the manifest at `path` (missing file → empty).
    pub fn load(path: &Path) -> Result<Self> {
        let manifest = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| Error::Privacy(format!("parse registry: {e}")))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Manifest::default(),
            Err(e) => return Err(Error::Io(e)),
        };
        Ok(Self {
            path: path.to_path_buf(),
            manifest,
        })
    }

    /// True when `key` is unknown or its stored hash differs from `content`'s.
    pub fn is_changed(&self, key: &str, content: &str) -> bool {
        match self.manifest.hashes.get(key) {
            Some(stored) => stored != &hash(content),
            None => true,
        }
    }

    /// Record `content`'s current hash under `key`.
    pub fn mark(&mut self, key: &str, content: &str) {
        self.manifest.hashes.insert(key.to_string(), hash(content));
    }

    /// Atomically write the manifest to disk as pretty JSON.
    pub fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|e| Error::Privacy(format!("serialize registry: {e}")))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Outcome of an incremental ingest.
pub enum Ingested {
    /// Content was new or changed; freshly anonymized.
    Scrubbed(String),
    /// Content unchanged since last ingest; the model was not called.
    Unchanged,
}

/// Runs the gateway over a source only when the registry says it changed.
pub struct Ingestor {
    gateway: Gateway,
    registry: Registry,
}

impl Ingestor {
    pub fn new(gateway: Gateway, registry: Registry) -> Self {
        Self { gateway, registry }
    }

    /// Anonymize `content` for `key` into `map` only if new/changed, updating the
    /// registry. Unchanged content returns `Unchanged` without a model call.
    pub async fn ingest(
        &mut self,
        key: &str,
        content: &str,
        map: &mut TokenMap,
    ) -> Result<Ingested> {
        if !self.registry.is_changed(key, content) {
            return Ok(Ingested::Unchanged);
        }
        let anonymized = self.gateway.anonymize(content, map).await?;
        self.registry.mark(key, content);
        Ok(Ingested::Scrubbed(anonymized))
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testing::MockAiClient;
    use crate::privacy::ner::NerEngine;

    #[test]
    fn is_changed_true_for_unknown_key() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::load(&dir.path().join("m.json")).unwrap();
        assert!(reg.is_changed("k", "content"));
    }

    #[test]
    fn is_changed_false_after_mark() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::load(&dir.path().join("m.json")).unwrap();
        reg.mark("k", "content");
        assert!(!reg.is_changed("k", "content"));
    }

    #[test]
    fn is_changed_true_after_content_edit() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::load(&dir.path().join("m.json")).unwrap();
        reg.mark("k", "content");
        assert!(reg.is_changed("k", "content edited"));
    }

    #[test]
    fn manifest_persists_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.json");
        {
            let mut reg = Registry::load(&path).unwrap();
            reg.mark("k", "content");
            reg.save().unwrap();
        }
        let reg = Registry::load(&path).unwrap();
        assert!(!reg.is_changed("k", "content"));
        assert!(reg.is_changed("k", "different"));
    }

    fn gateway_with(mock: MockAiClient) -> Gateway {
        Gateway::new(NerEngine::new(Box::new(mock)))
    }

    #[tokio::test]
    async fn unchanged_content_is_not_rescrubbed() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
        ]);
        let registry = Registry::load(&dir.path().join("m.json")).unwrap();
        let mut ing = Ingestor::new(gateway_with(mock.clone()), registry);
        let mut map = TokenMap::new();

        let first = ing.ingest("doc1", "hi Alice", &mut map).await.unwrap();
        assert!(matches!(first, Ingested::Scrubbed(_)));
        let second = ing.ingest("doc1", "hi Alice", &mut map).await.unwrap();
        assert!(matches!(second, Ingested::Unchanged));
        assert_eq!(
            mock.calls().len(),
            1,
            "unchanged content must not re-call the model"
        );
    }

    #[tokio::test]
    async fn changed_content_is_rescrubbed() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
        ]);
        let registry = Registry::load(&dir.path().join("m.json")).unwrap();
        let mut ing = Ingestor::new(gateway_with(mock.clone()), registry);
        let mut map = TokenMap::new();

        ing.ingest("doc1", "hi Alice", &mut map).await.unwrap();
        let second = ing
            .ingest("doc1", "hi Alice again", &mut map)
            .await
            .unwrap();
        assert!(matches!(second, Ingested::Scrubbed(_)));
        assert_eq!(mock.calls().len(), 2);
    }

    #[tokio::test]
    async fn rescrub_reuses_stable_token() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockAiClient::new(vec![
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
            r#"[{"text":"Alice","type":"person_name"}]"#.to_string(),
        ]);
        let registry = Registry::load(&dir.path().join("m.json")).unwrap();
        let mut ing = Ingestor::new(gateway_with(mock), registry);
        let mut map = TokenMap::new();

        let a = ing.ingest("doc1", "hi Alice", &mut map).await.unwrap();
        let b = ing.ingest("doc1", "bye Alice", &mut map).await.unwrap();
        match (a, b) {
            (Ingested::Scrubbed(first), Ingested::Scrubbed(second)) => {
                assert_eq!(first, "hi Person_1");
                assert_eq!(second, "bye Person_1");
            }
            _ => panic!("both ingests should have scrubbed"),
        }
    }
}
