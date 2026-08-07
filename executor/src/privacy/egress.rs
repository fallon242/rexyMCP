//! Executor egress protection (M45) — decide *when* to redact repo content on
//! its way to the executor model. Redaction engages only for a **cloud** executor
//! endpoint; a local/LAN endpoint keeps the real content (nothing leaves the
//! network). Classification errs toward cloud on ambiguity — misjudging a cloud
//! host as local would leak, misjudging a local host as cloud only over-redacts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::PrivacyConfig;

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
}
