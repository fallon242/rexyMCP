//! Encrypted-at-rest persistence for the reversible token dictionary — the
//! durable half of the "secure dictionary". The map is a PII honeypot, so it is
//! sealed with XChaCha20-Poly1305 under a local key file and kept in a
//! git-ignored directory: reversibility is the risk this buys, encryption is how
//! it is contained.

use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use super::tokenizer::TokenMap;
use crate::error::{Error, Result};

const KEY_FILE: &str = "key";
const VAULT_FILE: &str = "vault.enc";
const NONCE_LEN: usize = 24;

/// A durable, encrypted `TokenMap`. `open` loads (or creates) the key and any
/// existing vault; `save` re-seals the current map.
pub struct Vault {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
    map: TokenMap,
}

impl Vault {
    /// Open the vault at `dir`, creating the directory, its `.gitignore`, and a
    /// fresh key on first use, and loading any previously saved dictionary.
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        // The vault and its key must never be committed.
        fs::write(dir.join(".gitignore"), "*\n")?;

        let key = load_or_create_key(dir)?;
        let cipher = XChaCha20Poly1305::new(&key);

        let map = match fs::read(dir.join(VAULT_FILE)) {
            Ok(bytes) => decrypt_map(&cipher, &bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => TokenMap::new(),
            Err(e) => return Err(Error::Io(e)),
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            cipher,
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
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| Error::Privacy(format!("encrypt vault: {e}")))?;

        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(nonce.as_slice());
        blob.extend_from_slice(&ciphertext);

        let tmp = self.dir.join("vault.enc.tmp");
        fs::write(&tmp, &blob)?;
        fs::rename(&tmp, self.dir.join(VAULT_FILE))?;
        Ok(())
    }
}

fn load_or_create_key(dir: &Path) -> Result<Key> {
    let path = dir.join(KEY_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                return Err(Error::Privacy(format!(
                    "vault key at {} is {} bytes, expected 32",
                    path.display(),
                    bytes.len()
                )));
            }
            Ok(*Key::from_slice(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = XChaCha20Poly1305::generate_key(&mut OsRng);
            fs::write(&path, key.as_slice())?;
            set_owner_only(&path)?;
            Ok(key)
        }
        Err(e) => Err(Error::Io(e)),
    }
}

fn decrypt_map(cipher: &XChaCha20Poly1305, bytes: &[u8]) -> Result<TokenMap> {
    if bytes.len() < NONCE_LEN {
        return Err(Error::Privacy("vault file is truncated".into()));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let nonce = XNonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Privacy(format!("decrypt vault: {e}")))?;
    let entries = serde_json::from_slice(&plaintext)
        .map_err(|e| Error::Privacy(format!("parse vault: {e}")))?;
    Ok(TokenMap::from_entries(entries))
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<()> {
    Ok(())
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
        let mode = fs::metadata(dir.path().join(KEY_FILE))
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
