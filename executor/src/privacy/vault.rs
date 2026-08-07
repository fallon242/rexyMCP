//! Encrypted-at-rest persistence for the reversible token dictionary — the
//! durable half of the "secure dictionary". The map is a PII honeypot, so it is
//! sealed (XChaCha20-Poly1305 under a local key file, via [`super::seal`]) and
//! kept in a git-ignored directory: reversibility is the risk this buys,
//! encryption is how it is contained.

use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::Key;

use super::seal;
use super::tokenizer::TokenMap;
use crate::error::{Error, Result};

const VAULT_FILE: &str = "vault.enc";

/// A durable, encrypted `TokenMap`. `open` loads (or creates) the key and any
/// existing vault; `save` re-seals the current map.
pub struct Vault {
    dir: PathBuf,
    key: Key,
    map: TokenMap,
}

impl Vault {
    /// Open the vault at `dir`, creating the directory, its `.gitignore`, and a
    /// fresh key on first use, and loading any previously saved dictionary.
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        // The vault and its key must never be committed.
        fs::write(dir.join(".gitignore"), "*\n")?;

        let key = seal::load_or_create_key(dir)?;

        let map = match fs::read(dir.join(VAULT_FILE)) {
            Ok(blob) => {
                let plaintext = seal::unseal(&key, &blob)?;
                let entries = serde_json::from_slice(&plaintext)
                    .map_err(|e| Error::Privacy(format!("parse vault: {e}")))?;
                TokenMap::from_entries(entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => TokenMap::new(),
            Err(e) => return Err(Error::Io(e)),
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            key,
            map,
        })
    }

    pub fn map(&self) -> &TokenMap {
        &self.map
    }

    pub fn map_mut(&mut self) -> &mut TokenMap {
        &mut self.map
    }

    /// Encrypt the current dictionary and write it atomically to `vault.enc`.
    pub fn save(&self) -> Result<()> {
        let plaintext = serde_json::to_vec(&self.map.entries())
            .map_err(|e| Error::Privacy(format!("serialize vault: {e}")))?;
        let blob = seal::seal(&self.key, &plaintext)?;
        let tmp = self.dir.join("vault.enc.tmp");
        fs::write(&tmp, &blob)?;
        fs::rename(&tmp, self.dir.join(VAULT_FILE))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::PiiKind;

    #[test]
    fn roundtrips_token_map_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let token = {
            let mut v = Vault::open(dir.path()).unwrap();
            let t = v.map_mut().intern("Alice", PiiKind::PersonName);
            v.save().unwrap();
            t
        };
        let v2 = Vault::open(dir.path()).unwrap();
        assert_eq!(v2.map().reconstitute(&token), "Alice");
    }

    #[test]
    fn reopen_preserves_token_stability_and_advances_counter() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut v = Vault::open(dir.path()).unwrap();
            v.map_mut().intern("Alice", PiiKind::PersonName);
            v.map_mut().intern("Bob", PiiKind::PersonName);
            v.save().unwrap();
        }
        let mut v2 = Vault::open(dir.path()).unwrap();
        // Persisted original keeps its token; a new one advances past the max.
        assert_eq!(
            v2.map_mut().intern("Alice", PiiKind::PersonName),
            "Person_1"
        );
        assert_eq!(
            v2.map_mut().intern("Carol", PiiKind::PersonName),
            "Person_3"
        );
    }

    #[test]
    fn vault_file_is_encrypted_not_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = Vault::open(dir.path()).unwrap();
        v.map_mut().intern("Alice", PiiKind::PersonName);
        v.save().unwrap();
        let bytes = fs::read(dir.path().join(VAULT_FILE)).unwrap();
        assert!(!bytes.windows(5).any(|w| w == b"Alice"));
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        Vault::open(dir.path()).unwrap();
        let mode = fs::metadata(dir.path().join("key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn gitignore_written_into_vault_dir() {
        let dir = tempfile::tempdir().unwrap();
        Vault::open(dir.path()).unwrap();
        let gi = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gi.contains('*'));
    }
}
