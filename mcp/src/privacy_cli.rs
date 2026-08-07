//! `rexymcp anonymize` / `reconstitute` / `vault` — the human-facing CLI over the
//! M44 privacy gate. Anonymize scrubs PII (the local Qwen engine + deterministic
//! detectors) into a reversible encrypted vault; reconstitute reverses it; vault
//! reports counts without ever printing an original.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rexymcp_executor::config::{Config, PrivacyConfig};
use rexymcp_executor::privacy::gateway::Gateway;
use rexymcp_executor::privacy::ner::NerEngine;
use rexymcp_executor::privacy::vault::Vault;

pub struct CliArgs {
    pub config: PathBuf,
    pub repo: PathBuf,
    pub vault: Option<PathBuf>,
    pub input: Option<String>,
}

fn resolve_vault_dir(cfg: &PrivacyConfig, repo: &Path, override_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = override_dir {
        dir.to_path_buf()
    } else if let Some(dir) = &cfg.vault_dir {
        dir.clone()
    } else {
        repo.join(".rexymcp/vault")
    }
}

fn read_input(input: Option<&str>) -> Result<String> {
    match input {
        None | Some("-") => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading stdin")?;
            Ok(buf)
        }
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {path}")),
    }
}

pub async fn anonymize(args: CliArgs) -> Result<()> {
    let cfg = Config::load_with_env(&args.config)?;
    let text = read_input(args.input.as_deref())?;
    let gateway = Gateway::new(NerEngine::from_config(&cfg.privacy)?);
    let vault_dir = resolve_vault_dir(&cfg.privacy, &args.repo, args.vault.as_deref());
    let mut vault = Vault::open(&vault_dir)?;
    let anonymized = gateway.anonymize(&text, vault.map_mut()).await?;
    vault.save()?;
    print!("{anonymized}");
    Ok(())
}

pub fn reconstitute(args: CliArgs) -> Result<()> {
    let cfg = Config::load_with_env(&args.config)?;
    let text = read_input(args.input.as_deref())?;
    let vault_dir = resolve_vault_dir(&cfg.privacy, &args.repo, args.vault.as_deref());
    let vault = Vault::open(&vault_dir)?;
    print!("{}", vault.map().reconstitute(&text));
    Ok(())
}

pub fn vault_status(config: PathBuf, repo: PathBuf, vault: Option<PathBuf>) -> Result<()> {
    let cfg = Config::load_with_env(&config)?;
    let vault_dir = resolve_vault_dir(&cfg.privacy, &repo, vault.as_deref());
    let opened = Vault::open(&vault_dir)?;
    let entries = opened.map().entries();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &entries {
        *counts.entry(entry.kind.token_prefix()).or_insert(0) += 1;
    }
    println!("vault:   {}", vault_dir.display());
    println!("entries: {}", entries.len());
    for (kind, n) in &counts {
        println!("  {kind}: {n}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_dir_prefers_explicit_override() {
        let cfg = PrivacyConfig::default();
        let dir = resolve_vault_dir(&cfg, Path::new("/repo"), Some(Path::new("/custom")));
        assert_eq!(dir, PathBuf::from("/custom"));
    }

    #[test]
    fn vault_dir_falls_back_to_config() {
        let cfg = PrivacyConfig {
            vault_dir: Some(PathBuf::from("/from/config")),
            ..Default::default()
        };
        let dir = resolve_vault_dir(&cfg, Path::new("/repo"), None);
        assert_eq!(dir, PathBuf::from("/from/config"));
    }

    #[test]
    fn vault_dir_defaults_under_repo() {
        let cfg = PrivacyConfig::default();
        let dir = resolve_vault_dir(&cfg, Path::new("/repo"), None);
        assert_eq!(dir, PathBuf::from("/repo/.rexymcp/vault"));
    }
}
