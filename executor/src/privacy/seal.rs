//! Reusable authenticated encryption for the privacy module's at-rest secrets —
//! the reversible vault dictionary and the executor-egress pre-scan index, both
//! PII honeypots. XChaCha20-Poly1305 under a local `0600` key file; on-disk form
//! is `[24-byte XNonce ‖ ciphertext]`.

use std::fs;
use std::path::Path;

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use crate::error::{Error, Result};

const KEY_FILE: &str = "key";
const NONCE_LEN: usize = 24;

/// Load the 32-byte key at `dir/key`, creating it (`0600` on unix) on first use.
pub fn load_or_create_key(dir: &Path) -> Result<Key> {
    let path = dir.join(KEY_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                return Err(Error::Privacy(format!(
                    "key at {} is {} bytes, expected 32",
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

/// Encrypt `plaintext` under `key`, returning `[nonce ‖ ciphertext]`.
pub fn seal(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key);
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| Error::Privacy(format!("seal: {e}")))?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(nonce.as_slice());
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt a `[nonce ‖ ciphertext]` blob under `key`.
pub fn unseal(key: &Key, blob: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < NONCE_LEN {
        return Err(Error::Privacy("sealed blob is truncated".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key);
    let nonce = XNonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Privacy(format!("unseal: {e}")))
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

    #[test]
    fn seal_unseal_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let key = load_or_create_key(dir.path()).unwrap();
        let blob = seal(&key, b"hello pii").unwrap();
        assert_ne!(
            &blob[24..],
            b"hello pii",
            "ciphertext must not be plaintext"
        );
        assert_eq!(unseal(&key, &blob).unwrap(), b"hello pii");
    }

    #[cfg(unix)]
    #[test]
    fn key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        load_or_create_key(dir.path()).unwrap();
        let mode = fs::metadata(dir.path().join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
