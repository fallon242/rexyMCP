//! Executor egress protection (M45) — decide *when* to redact repo content on
//! its way to the executor model. Redaction engages only for a **cloud** executor
//! endpoint; a local/LAN endpoint keeps the real content (nothing leaves the
//! network). Classification errs toward cloud on ambiguity — misjudging a cloud
//! host as local would leak, misjudging a local host as cloud only over-redacts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::PiiKind;
use super::ner::NerEngine;
use super::prescan::{PiiIndex, build_pii_index};
use super::registry::Registry;
use super::terms::LiteralTerms;
use crate::config::PrivacyConfig;
use crate::error::Result;

/// Lowercase `s` and drop everything that is not a letter or a digit, so
/// `rexyMCP`, `rexy-mcp` and `rexymcp` all normalize to `rexymcp`.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The names a project calls itself: its directory name, plus whatever its
/// package manifests declare. Normalized, so a term can be compared directly.
/// Entries shorter than two characters are dropped as too generic.
pub fn project_vocabulary(root: &Path, files: &[(PathBuf, String)]) -> HashSet<String> {
    let mut vocab = HashSet::new();
    if let Some(name) = root.file_name() {
        insert_normalized(&mut vocab, &name.to_string_lossy());
    }
    for (path, content) in files {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let name = match file_name {
            "Cargo.toml" | "pyproject.toml" => quoted_name_field(content),
            "package.json" => serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string)),
            "go.mod" => content
                .lines()
                .find_map(|l| l.trim().strip_prefix("module "))
                .map(|m| {
                    m.split('/')
                        .next_back()
                        .map(str::to_string)
                        .unwrap_or(m.to_string())
                }),
            _ => None,
        };
        if let Some(name) = name {
            insert_normalized(&mut vocab, &name);
        }
    }
    vocab
}

fn insert_normalized(vocab: &mut HashSet<String>, name: &str) {
    let normalized = normalize(name);
    if normalized.len() >= 2 {
        vocab.insert(normalized);
    }
}

/// The value of a `name = "…"` style line (Cargo.toml / pyproject.toml): what is
/// between the first pair of double quotes after the `=`.
fn quoted_name_field(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("name") {
            return None;
        }
        let after = trimmed.strip_prefix("name")?;
        let rest = after.split_once('=')?.1;
        let start = rest.find('"')?;
        let tail = &rest[start + 1..];
        let end = tail.find('"')?;
        Some(tail[..end].to_string())
    })
}

/// True when `term` is the project naming itself rather than PII: the same name
/// after normalization, or an identifier built from it (`rexymcp-executor`).
fn is_project_name(term: &str, vocabulary: &HashSet<String>) -> bool {
    let t = normalize(term);
    if t.is_empty() {
        return false;
    }
    if vocabulary.contains(&t) {
        return true;
    }
    vocabulary
        .iter()
        .any(|v| v.len() >= 4 && t.contains(v.as_str()))
}

/// True when `base_url`'s host is clearly local: `localhost`, a loopback/RFC-1918
/// IP, or a `.local` / `.lan` / `.internal` hostname. Every other host — any
/// public domain or IP — is cloud.
pub fn endpoint_is_local(base_url: &str) -> bool {
    is_local_host(host_of(base_url))
}

/// Resolve whether outbound executor content should be redacted:
/// off when the gate is disabled; otherwise the explicit
/// `redact_executor_egress` override, else auto (redact iff the endpoint is cloud).
pub fn should_redact_egress(privacy: &PrivacyConfig, base_url: &str) -> bool {
    if !privacy.enabled {
        return false;
    }
    match privacy.redact_executor_egress {
        Some(force) => force,
        None => !endpoint_is_local(base_url),
    }
}

/// Refuse an edit to a PII-bearing file (M45 write-guard). A cloud executor only
/// ever sees a file's **redacted** contents, so it must not overwrite one — it
/// would replace real data with fabrication (the phase-06b failure). `edit_target`
/// is the resolved `write_file`/`patch` target (`None` for non-edit calls);
/// `pii_files` are the resolved paths the pre-scan found to contain PII (empty =
/// protection off). `None` = allowed.
pub fn pii_write_refusal(
    edit_target: Option<&Path>,
    pii_files: &HashSet<PathBuf>,
) -> Option<String> {
    let path = edit_target?;
    if pii_files.contains(path) {
        Some(format!(
            "refusing to edit {}: it contains PII, and the executor is a cloud model that only \
             sees its redacted contents. Edit this file manually, or run the phase on a local \
             executor.",
            path.display()
        ))
    } else {
        None
    }
}

/// Walk `root` for text files to pre-scan, honoring `.gitignore` / `.ignore` (so
/// `.git`, `target`, `.rexymcp`, … are skipped) and hidden files. Binary and
/// oversized (>1 MiB) files are skipped. Paths are absolute (as `root` is),
/// matching the loop's resolved edit targets.
pub fn scan_repo_files(root: &Path, globs: &[String]) -> Vec<(PathBuf, String)> {
    const MAX_BYTES: u64 = 1_048_576;
    let globset = build_globset(globs);
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if let Some(gs) = &globset {
            let rel = path.strip_prefix(root).unwrap_or(path);
            if !gs.is_match(rel) {
                continue;
            }
        }
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_BYTES {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            out.push((path.to_path_buf(), content));
        }
    }
    out
}

/// Build a `GlobSet` from repo-relative patterns; `None` when empty (= match
/// everything). Invalid patterns are skipped.
fn build_globset(globs: &[String]) -> Option<globset::GlobSet> {
    if globs.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in globs {
        if let Ok(glob) = globset::Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// The outbound-redaction inputs for one dispatch: the NER pre-scan dictionary,
/// the PII-bearing file set (for the write-guard), and the literal term file
/// (`terms_file`, which is empty when unset).
#[derive(Debug, Default)]
pub struct EgressIndex {
    pub terms: Vec<(String, PiiKind)>,
    pub pii_files: HashSet<PathBuf>,
    pub literal: LiteralTerms,
}

/// Pre-scan the repo for PII: build the redaction term dictionary, the
/// PII-bearing file set, and the literal term file. The term file is loaded
/// **first** so a bad file is reported even when the NER engine is
/// misconfigured. Errors if the `[privacy]` NER engine is unset (the caller may
/// then degrade to deterministic-only live redaction). NOTE: index persistence
/// across dispatches is a follow-up, so this currently scans every file on each
/// dispatch (the registry marks hashes but the prior index is not persisted).
pub async fn build_egress_index(root: &Path, privacy: &PrivacyConfig) -> Result<EgressIndex> {
    // A term file that fails to load stops the dispatch: an operator who believes
    // they are protected is worse off than one who is not.
    let literal = match &privacy.terms_file {
        None => LiteralTerms::default(),
        Some(p) if p.is_absolute() => LiteralTerms::load(p)?,
        Some(p) => LiteralTerms::load(&root.join(p))?,
    };
    let ner = NerEngine::from_config(privacy)?;
    let files = scan_repo_files(root, &privacy.scan_globs);
    // Persist the index + registry under the vault dir (M46) so an unchanged file
    // reuses its prior entry — no NER call — on the next dispatch.
    let vault_dir = privacy
        .vault_dir
        .clone()
        .unwrap_or_else(|| root.join(".rexymcp/vault"));
    let prior = PiiIndex::load(&vault_dir)?;
    let mut registry = Registry::load(&vault_dir.join("egress-registry.json"))?;
    let index = build_pii_index(&files, &ner, &mut registry, &prior).await?;
    index.save(&vault_dir)?;
    registry.save()?;
    // Finding 9: the project's own name is not PII. Redacting it hands the
    // executor `[REDACTED:org]-executor` instead of a crate name it can use.
    let vocabulary = project_vocabulary(root, &files);
    let index = index.retaining(|term| !is_project_name(term, &vocabulary));
    let terms = index.redaction_terms();
    let pii_files = index.files().cloned().collect();
    Ok(EgressIndex {
        terms,
        pii_files,
        literal,
    })
}

fn host_of(url: &str) -> &str {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme.split('/').next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal, e.g. [::1]:8080
        return rest.split(']').next().unwrap_or(rest);
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_local_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback() || matches!(ip, std::net::IpAddr::V4(v4) if v4.is_private());
    }
    let lower = host.to_ascii_lowercase();
    lower.ends_with(".local") || lower.ends_with(".lan") || lower.ends_with(".internal")
}

#[cfg(test)]
mod tests {
    use crate::error::Error;

    use super::*;

    #[test]
    fn local_hosts_classified_local() {
        for url in [
            "http://localhost:1234/v1",
            "http://127.0.0.1:8080/v1",
            "http://192.168.1.10:8080/v1",
            "http://10.0.0.5/v1",
            "http://172.20.1.1:11434/v1",
            "http://[::1]:8080/v1",
            "http://ner.local:8080/v1",
        ] {
            assert!(endpoint_is_local(url), "expected local: {url}");
        }
    }

    #[test]
    fn cloud_hosts_classified_cloud() {
        for url in [
            "https://api.deepseek.com",
            "https://api.openai.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1", // just outside the 172.16–31 private range
        ] {
            assert!(!endpoint_is_local(url), "expected cloud: {url}");
        }
    }

    #[test]
    fn strips_scheme_port_and_path() {
        assert!(endpoint_is_local("192.168.1.1"));
        assert!(!endpoint_is_local("api.deepseek.com/chat/completions"));
    }

    #[test]
    fn ipv6_loopback_is_local() {
        assert!(endpoint_is_local("http://[::1]:9000"));
    }

    #[test]
    fn private_range_boundaries() {
        assert!(endpoint_is_local("http://172.16.0.1/v1"));
        assert!(endpoint_is_local("http://172.31.255.255/v1"));
        assert!(!endpoint_is_local("http://172.15.0.1/v1"));
        assert!(!endpoint_is_local("http://172.32.0.1/v1"));
    }

    fn privacy(enabled: bool, force: Option<bool>) -> PrivacyConfig {
        PrivacyConfig {
            enabled,
            redact_executor_egress: force,
            ..Default::default()
        }
    }

    #[test]
    fn disabled_gate_never_redacts() {
        assert!(!should_redact_egress(
            &privacy(false, None),
            "https://api.deepseek.com"
        ));
    }

    #[test]
    fn auto_redacts_cloud_not_local() {
        assert!(should_redact_egress(
            &privacy(true, None),
            "https://api.deepseek.com"
        ));
        assert!(!should_redact_egress(
            &privacy(true, None),
            "http://192.168.1.10:8080/v1"
        ));
    }

    #[test]
    fn explicit_override_wins() {
        // Force on even for a local endpoint.
        assert!(should_redact_egress(
            &privacy(true, Some(true)),
            "http://localhost:1234/v1"
        ));
        // Force off even for a cloud endpoint.
        assert!(!should_redact_egress(
            &privacy(true, Some(false)),
            "https://api.deepseek.com"
        ));
    }

    #[test]
    fn write_guard_refuses_pii_file() {
        let mut pii = HashSet::new();
        pii.insert(PathBuf::from("/repo/data/users.json"));
        assert!(pii_write_refusal(Some(Path::new("/repo/data/users.json")), &pii).is_some());
    }

    #[test]
    fn write_guard_allows_clean_file() {
        let mut pii = HashSet::new();
        pii.insert(PathBuf::from("/repo/data/users.json"));
        assert!(pii_write_refusal(Some(Path::new("/repo/src/main.rs")), &pii).is_none());
    }

    #[test]
    fn write_guard_allows_non_edit_call() {
        let pii = HashSet::new();
        assert!(pii_write_refusal(None, &pii).is_none());
    }

    #[test]
    fn write_guard_empty_set_never_refuses() {
        let pii: HashSet<PathBuf> = HashSet::new();
        assert!(pii_write_refusal(Some(Path::new("/repo/data/users.json")), &pii).is_none());
    }

    #[tokio::test]
    #[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]
    async fn live_build_egress_index_finds_pii() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("data.json"),
            r#"{"owner": "John Smith", "email": "jane@acme.com"}"#,
        )
        .unwrap();
        let privacy = PrivacyConfig {
            enabled: true,
            engine_base_url: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_URL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_URL to run this live test"),
            ),
            engine_model: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_MODEL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_MODEL to run this live test"),
            ),
            ..Default::default()
        };

        let idx = build_egress_index(dir.path(), &privacy).await.unwrap();
        let terms = &idx.terms;
        let files = &idx.pii_files;

        let term_strs: Vec<&str> = terms.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            term_strs.iter().any(|t| t.contains("John")),
            "expected a name term, got {term_strs:?}"
        );
        assert!(
            term_strs.contains(&"jane@acme.com"),
            "expected the email term, got {term_strs:?}"
        );
        assert!(
            files.iter().any(|p| p.ends_with("data.json")),
            "data.json must be flagged PII-bearing"
        );
    }

    #[test]
    fn scan_reads_text_files_and_skips_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "hello alice").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.txt"), "bob").unwrap();
        std::fs::write(root.join(".ignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(root.join("ignored")).unwrap();
        std::fs::write(root.join("ignored/secret.txt"), "carol").unwrap();

        let names: Vec<String> = scan_repo_files(root, &[])
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"b.txt".to_string()));
        assert!(
            !names.iter().any(|n| n == "secret.txt"),
            "an ignored file must be skipped: {names:?}"
        );
    }

    #[test]
    fn scan_globs_limit_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/users.json"), "alice").unwrap();
        std::fs::write(root.join("main.rs"), "code").unwrap();

        let rel: Vec<String> = scan_repo_files(root, &["data/**".to_string()])
            .iter()
            .map(|(p, _)| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(rel.iter().any(|n| n.contains("users.json")), "{rel:?}");
        assert!(
            !rel.iter().any(|n| n.contains("main.rs")),
            "a non-matching file must be excluded: {rel:?}"
        );
    }

    #[test]
    fn project_vocabulary_collects_repo_and_manifest_names() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("acme-widgets");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"widgetcore\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("package.json"),
            "{\"name\": \"@acme/widgets-ui\", \"version\": \"1.0.0\"}",
        )
        .unwrap();
        std::fs::write(
            repo.join("pyproject.toml"),
            "[project]\nname = \"widgets_py\"\n",
        )
        .unwrap();
        std::fs::write(repo.join("go.mod"), "module github.com/acme/widgetsvc\n").unwrap();

        let files = scan_repo_files(&repo, &[]);
        let vocab = project_vocabulary(&repo, &files);

        assert!(vocab.contains("acmewidgets"), "got {vocab:?}");
        assert!(vocab.contains("widgetcore"), "got {vocab:?}");
        assert!(vocab.contains("acmewidgetsui"), "got {vocab:?}");
        assert!(vocab.contains("widgetspy"), "got {vocab:?}");
        assert!(vocab.contains("widgetsvc"), "got {vocab:?}");
        assert!(!vocab.contains("githubcomacmewidgetsvc"), "got {vocab:?}");
        assert!(!vocab.contains("version"), "got {vocab:?}");
    }

    #[test]
    fn project_vocabulary_ignores_unparseable_manifests() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("ab");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("package.json"), "not json at all").unwrap();

        let files = scan_repo_files(&repo, &[]);
        let vocab = project_vocabulary(&repo, &files);

        assert!(
            vocab.is_empty() || vocab == ["ab".to_string()].into_iter().collect(),
            "got {vocab:?}"
        );
    }

    #[test]
    fn is_project_name_drops_identifiers_not_people() {
        let vocab = HashSet::from(["rexymcp".to_string()]);
        assert!(is_project_name("rexymcp", &vocab));
        assert!(is_project_name("rexyMCP", &vocab));
        assert!(is_project_name("rexy-mcp", &vocab));
        assert!(is_project_name("rexymcp-executor", &vocab));
        assert!(is_project_name("rexymcp.toml", &vocab));
        assert!(!is_project_name("Alice", &vocab));
        assert!(!is_project_name("Rosa", &vocab));
        assert!(!is_project_name("Bob Rexy", &vocab));
        assert!(!is_project_name("mcp", &vocab));
        assert!(!is_project_name("", &vocab));

        let vocab2 = HashSet::from(["prosaictool".to_string(), "crs".to_string()]);
        assert!(
            !is_project_name("Rosa", &vocab2),
            "a vocabulary entry containing a term must not drop it"
        );
        assert!(
            !is_project_name("Crsanova", &vocab2),
            "the 4-character floor keeps `crs` from matching inside it"
        );
        assert!(is_project_name("crs", &vocab2));
    }

    #[tokio::test]
    #[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]
    async fn live_build_egress_index_keeps_project_names_out() {
        let base_url =
            std::env::var("REXYMCP_PRIVACY_ENGINE_URL").expect("REXYMCP_PRIVACY_ENGINE_URL");
        let model =
            std::env::var("REXYMCP_PRIVACY_ENGINE_MODEL").expect("REXYMCP_PRIVACY_ENGINE_MODEL");

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("rexymcp-sample");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"rexymcp-sample\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("notes.md"),
            "The rexymcp-sample pipeline was reviewed by Alice Fernandez on Tuesday.\n\
             See rexymcp-sample/README for the crate layout.\n",
        )
        .unwrap();

        let mut privacy = crate::config::PrivacyConfig {
            enabled: true,
            engine_base_url: Some(base_url),
            engine_model: Some(model),
            vault_dir: None,
            redact_executor_egress: None,
            scan_globs: vec![],
            terms_file: None,
        };
        privacy.vault_dir = Some(tmp.path().join("vault"));

        let idx = build_egress_index(&repo, &privacy).await.unwrap();
        let terms = idx.terms;
        let normalized: Vec<String> = terms.iter().map(|(t, _)| normalize(t)).collect();
        eprintln!("live terms: {terms:?}");

        assert!(
            terms.iter().any(|(t, _)| t.contains("Alice")),
            "the pre-scan must still find the person's name, got {terms:?}"
        );
        assert!(
            !normalized.iter().any(|n| n.contains("rexymcpsample")),
            "no project name may survive the filter, got {normalized:?}"
        );
    }

    fn hermetic_privacy(terms_file: Option<std::path::PathBuf>) -> PrivacyConfig {
        PrivacyConfig {
            engine_base_url: Some("http://localhost:9/v1".to_string()),
            engine_model: Some("m".to_string()),
            scan_globs: vec!["no-such-dir/**".to_string()],
            terms_file,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn unset_terms_file_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut privacy = hermetic_privacy(None);
        privacy.vault_dir = Some(dir.path().join("vault"));
        let idx = build_egress_index(dir.path(), &privacy)
            .await
            .expect("no NER call is made when the glob matches no file");
        let s = "Plant Nine and PLN";
        assert_eq!(idx.literal.mask(s), s, "an unset term file must not mask");
        assert!(
            idx.terms.is_empty(),
            "the glob matches no file, so no terms"
        );
    }

    #[tokio::test]
    async fn missing_terms_file_fails_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-terms.json");
        let mut privacy = hermetic_privacy(Some(missing.clone()));
        privacy.vault_dir = Some(dir.path().join("vault"));
        let err = build_egress_index(dir.path(), &privacy).await.expect_err(
            "a missing term file must stop the dispatch, even with the engine misconfigured",
        );
        let Error::Privacy(m) = err else {
            panic!("expected Error::Privacy, got {err:?}");
        };
        assert!(
            m.contains("no-such-terms.json"),
            "the path must be named: {m}"
        );
    }
}
