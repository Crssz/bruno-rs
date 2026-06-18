//! A small encrypted secrets vault.
//!
//! Secret values are stored AES-256-GCM-encrypted in `~/.bruno-rs/vault.enc`,
//! with the 256-bit key stretched from a master password via iterated SHA-256
//! over a random per-vault salt. The on-disk layout is
//! `[16-byte salt][12-byte nonce][ciphertext]`. The plaintext is a JSON map of
//! `name -> secret value`. Values are only ever held in memory once unlocked.

use std::collections::HashMap;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use sha2::{Digest, Sha256};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
/// SHA-256 stretch rounds. Not Argon2, but it makes a brute-force meaningfully
/// more expensive than a single hash while staying dependency-light.
const ITERS: u32 = 200_000;

/// `~/.bruno-rs/vault.enc`, creating the parent dir on demand.
fn vault_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or("no home directory")?;
    let dir = PathBuf::from(home).join(".bruno-rs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("vault.enc"))
}

/// Whether a vault file already exists on disk.
pub fn exists() -> bool {
    vault_path().map(|p| p.exists()).unwrap_or(false)
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut out = {
        let mut h = Sha256::new();
        h.update(salt);
        h.update(password.as_bytes());
        h.finalize()
    };
    for _ in 1..ITERS {
        let mut h = Sha256::new();
        h.update(out);
        out = h.finalize();
    }
    out.into()
}

fn encrypt(password: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| "encryption failed".to_string())?;
    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < SALT_LEN + NONCE_LEN {
        return Err("vault file is corrupt".to_string());
    }
    let (salt, rest) = data.split_at(SALT_LEN);
    let (nonce, ct) = rest.split_at(NONCE_LEN);
    let key = derive_key(password, salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "wrong password or corrupt vault".to_string())
}

/// Unlock and read the vault. A missing file unlocks to an empty vault (so a
/// first unlock establishes the master password on the next save).
pub fn load(password: &str) -> Result<HashMap<String, String>, String> {
    let path = vault_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let pt = decrypt(password, &data)?;
    serde_json::from_slice(&pt).map_err(|e| e.to_string())
}

/// Encrypt and write the vault under `password`.
pub fn save(password: &str, map: &HashMap<String, String>) -> Result<(), String> {
    let path = vault_path()?;
    let pt = serde_json::to_vec(map).map_err(|e| e.to_string())?;
    let data = encrypt(password, &pt)?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_wrong_password() {
        let pt = b"{\"API_KEY\":\"s3cr3t\"}";
        let blob = encrypt("hunter2", pt).unwrap();
        // Layout: salt + nonce + ciphertext, and ciphertext isn't the plaintext.
        assert!(blob.len() > SALT_LEN + NONCE_LEN);
        assert!(!blob.windows(pt.len()).any(|w| w == pt));
        assert_eq!(decrypt("hunter2", &blob).unwrap(), pt);
        assert!(decrypt("wrong", &blob).is_err());
    }

    #[test]
    fn derive_key_is_deterministic_and_salt_sensitive() {
        let k1 = derive_key("pw", b"saltsaltsaltsalt");
        let k2 = derive_key("pw", b"saltsaltsaltsalt");
        let k3 = derive_key("pw", b"DIFFERENTsaltxxx");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
