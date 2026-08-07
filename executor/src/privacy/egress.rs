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
use crate::config::PrivacyConfig;
use crate::error::Result;

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
pub fn scan_repo_files(root: &Path) -> Vec<(PathBuf, String)> {
    const MAX_BYTES: u64 = 1_048_576;
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_BYTES {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            out.push((path.to_path_buf(), content));
        }
    }
    out
}

/// Pre-scan the repo for PII: returns the outbound-redaction term dictionary +
/// the PII-bearing file set. Errors if the `[privacy]` NER engine is unset (the
/// caller may then degrade to deterministic-only live redaction). NOTE: index
/// persistence across dispatches is a follow-up, so this currently scans every
/// file on each dispatch (the registry marks hashes but the prior index is not
/// persisted).
pub async fn build_egress_index(
    root: &Path,
    privacy: &PrivacyConfig,
) -> Result<(Vec<(String, PiiKind)>, HashSet<PathBuf>)> {
    let ner = NerEngine::from_config(privacy)?;
    let files = scan_repo_files(root);
    let mut registry = Registry::load(&root.join(".rexymcp/egress-prescan.json"))?;
    let index = build_pii_index(&files, &ner, &mut registry, &PiiIndex::empty()).await?;
    let terms = index.redaction_terms();
    let pii_files = index.files().cloned().collect();
    Ok((terms, pii_files))
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
    use super::*;

    #[test]
    fn local_hosts_classified_local() {
        for url in [
            "http://localhost:1234/v1",
            "http://127.0.0.1:8080/v1",
            "http://192.168.50.138:8080/v1",
            "http://10.0.0.5/v1",
            "http://172.20.1.1:11434/v1",
            "http://[::1]:8080/v1",
            "http://qwen.local:8080/v1",
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
            "http://192.168.50.138:8080/v1"
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
    #[ignore = "live: needs Qwen at the [privacy] engine endpoint; run with --ignored"]
    async fn live_build_egress_index_finds_pii() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("data.json"),
            r#"{"owner": "John Smith", "email": "jane@acme.com"}"#,
        )
        .unwrap();
        let privacy = PrivacyConfig {
            enabled: true,
            engine_base_url: Some("http://192.168.50.138:8080/v1".to_string()),
            engine_model: Some("qwen3.5-9b".to_string()),
            ..Default::default()
        };

        let (terms, files) = build_egress_index(dir.path(), &privacy).await.unwrap();

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

        let names: Vec<String> = scan_repo_files(root)
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
}
