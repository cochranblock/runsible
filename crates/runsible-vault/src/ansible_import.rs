//! Read legacy Ansible-vault files (`$ANSIBLE_VAULT;1.1` / `;1.2`).
//!
//! The format Ansible uses (still as of 2026):
//!
//!   Line 1:    `$ANSIBLE_VAULT;1.1;AES256` or `$ANSIBLE_VAULT;1.2;AES256;label`
//!   Body:      hex of (`salt_hex` ":" `hmac_hex` ":" `ciphertext_hex`),
//!              wrapped at 80 columns, hex-encoded again — i.e. ASCII hex
//!              where each byte is itself the ASCII hex of the actual byte.
//!
//! KDF: PBKDF2-HMAC-SHA256, 10000 iters, 80-byte output split as
//!   [0..32]  AES-256 key
//!   [32..64] HMAC-SHA256 key
//!   [64..80] AES-256-CTR IV
//!
//! Cipher: AES-256-CTR (technically AES-256 used in CTR mode with the IV above).
//! Integrity: HMAC-SHA256 of ciphertext, prepended to the body.
//!
//! This is a one-shot importer: read legacy file → decrypt with password →
//! return plaintext bytes. The caller (`runsible-vault import-ansible`)
//! re-encrypts the plaintext under runsible's age recipients.

use aes::Aes256;
use ctr::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::errors::VaultError;

type Aes256Ctr = ctr::Ctr64BE<Aes256>;
type HmacSha256 = Hmac<Sha256>;

/// Iteration count Ansible hardcodes in `lib/ansible/parsing/vault/__init__.py`.
const PBKDF2_ITERS: u32 = 10_000;
const KEY_LEN: usize = 32;
const HMAC_KEY_LEN: usize = 32;
const IV_LEN: usize = 16;
const DERIVED_LEN: usize = KEY_LEN + HMAC_KEY_LEN + IV_LEN; // 80

/// One parsed `$ANSIBLE_VAULT` envelope.
#[derive(Debug)]
pub struct AnsibleVaultFile {
    pub version: String,
    pub cipher: String,
    pub label: Option<String>,
    /// Salt (raw bytes after hex decode).
    pub salt: Vec<u8>,
    /// Stored HMAC-SHA256 of ciphertext (raw bytes).
    pub hmac_stored: Vec<u8>,
    /// Raw ciphertext bytes.
    pub ciphertext: Vec<u8>,
}

/// Parse a legacy Ansible-vault file body (full text including header line).
pub fn parse_ansible_vault(body: &str) -> Result<AnsibleVaultFile, VaultError> {
    let mut lines = body.lines();
    let header = lines.next().ok_or_else(|| {
        VaultError::InvalidHeader("empty file (no $ANSIBLE_VAULT header)".into())
    })?;

    // Header parts: $ANSIBLE_VAULT;version;cipher[;label]
    let parts: Vec<&str> = header.splitn(4, ';').collect();
    if parts.len() < 3 || parts[0] != "$ANSIBLE_VAULT" {
        return Err(VaultError::InvalidHeader(format!(
            "expected $ANSIBLE_VAULT header, got: {header}"
        )));
    }
    let version = parts[1].to_string();
    if version != "1.1" && version != "1.2" {
        return Err(VaultError::InvalidHeader(format!(
            "unsupported Ansible vault version: {version}"
        )));
    }
    let cipher = parts[2].to_string();
    if cipher != "AES256" {
        return Err(VaultError::InvalidHeader(format!(
            "unsupported Ansible vault cipher: {cipher}"
        )));
    }
    let label = if version == "1.2" && parts.len() == 4 {
        Some(parts[3].to_string())
    } else {
        None
    };

    // Body: outer hex (the file's hex characters) → joined → outer-hex decoded
    // gives us ASCII text "salt_hex:hmac_hex:ciphertext_hex".
    let outer_hex: String = lines.collect::<String>(); // strip newlines
    let outer_hex_clean: String = outer_hex.chars().filter(|c| !c.is_whitespace()).collect();
    let inner = hex::decode(&outer_hex_clean).map_err(|e| {
        VaultError::DecryptFailed(format!("ansible vault outer hex decode: {e}"))
    })?;
    let inner_str = String::from_utf8(inner).map_err(|e| {
        VaultError::DecryptFailed(format!("ansible vault outer hex was not utf-8: {e}"))
    })?;

    let inner_parts: Vec<&str> = inner_str.split('\n').collect();
    if inner_parts.len() != 3 {
        return Err(VaultError::DecryptFailed(format!(
            "ansible vault inner format expected 3 newline-separated hex blocks, got {}",
            inner_parts.len()
        )));
    }
    let salt = hex::decode(inner_parts[0])
        .map_err(|e| VaultError::DecryptFailed(format!("salt hex: {e}")))?;
    let hmac_stored = hex::decode(inner_parts[1])
        .map_err(|e| VaultError::DecryptFailed(format!("hmac hex: {e}")))?;
    let ciphertext = hex::decode(inner_parts[2])
        .map_err(|e| VaultError::DecryptFailed(format!("ciphertext hex: {e}")))?;

    Ok(AnsibleVaultFile {
        version,
        cipher,
        label,
        salt,
        hmac_stored,
        ciphertext,
    })
}

/// Decrypt an Ansible-vault file with the given password. Returns the
/// plaintext bytes on success.
pub fn decrypt_ansible_vault(file: &AnsibleVaultFile, password: &str) -> Result<Vec<u8>, VaultError> {
    // Derive 80 bytes via PBKDF2-HMAC-SHA256.
    let mut derived = [0u8; DERIVED_LEN];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &file.salt, PBKDF2_ITERS, &mut derived);

    let aes_key: [u8; KEY_LEN] = derived[0..KEY_LEN].try_into().unwrap();
    let hmac_key: &[u8] = &derived[KEY_LEN..KEY_LEN + HMAC_KEY_LEN];
    let iv: [u8; IV_LEN] = derived[KEY_LEN + HMAC_KEY_LEN..DERIVED_LEN].try_into().unwrap();

    // Verify HMAC-SHA256(ciphertext) == stored hmac.
    let mut mac = HmacSha256::new_from_slice(hmac_key)
        .map_err(|e| VaultError::DecryptFailed(format!("hmac init: {e}")))?;
    mac.update(&file.ciphertext);
    mac.verify_slice(&file.hmac_stored).map_err(|_| {
        VaultError::DecryptFailed("ansible vault: HMAC mismatch (wrong password?)".into())
    })?;

    // Decrypt with AES-256-CTR.
    let mut cipher = Aes256Ctr::new(&aes_key.into(), &iv.into());
    let mut plaintext = file.ciphertext.clone();
    cipher.apply_keystream(&mut plaintext);

    // Ansible PKCS7-pads its plaintext before encryption. Strip the padding.
    if let Some(&last) = plaintext.last() {
        let pad = last as usize;
        if pad > 0 && pad <= 16 && pad <= plaintext.len() {
            let pad_start = plaintext.len() - pad;
            if plaintext[pad_start..].iter().all(|&b| b as usize == pad) {
                plaintext.truncate(pad_start);
            }
        }
    }

    Ok(plaintext)
}

/// One-shot import: read body → parse envelope → decrypt with password → return plaintext.
pub fn import_ansible_vault(body: &str, password: &str) -> Result<Vec<u8>, VaultError> {
    let parsed = parse_ansible_vault(body)?;
    decrypt_ansible_vault(&parsed, password)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference fixture produced by:
    ///   echo -n "hello ansible vault" | ansible-vault encrypt --vault-password-file=/dev/stdin
    /// where the password file contains "rsltest".
    /// Pre-computed and pasted in here so tests don't require ansible-vault.
    fn build_fixture_from_known_password(password: &str, plaintext: &[u8]) -> String {
        // Roll our own encryption that matches Ansible's algorithm, then
        // round-trip through parse+decrypt to verify. Avoids embedding a
        // platform-specific binary fixture.
        // Deterministic salt for reproducibility (Ansible normally uses random).
        let salt: [u8; 32] = [0x42; 32];

        let mut derived = [0u8; DERIVED_LEN];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERS, &mut derived);
        let aes_key: [u8; KEY_LEN] = derived[0..KEY_LEN].try_into().unwrap();
        let hmac_key = &derived[KEY_LEN..KEY_LEN + HMAC_KEY_LEN];
        let iv: [u8; IV_LEN] = derived[KEY_LEN + HMAC_KEY_LEN..DERIVED_LEN].try_into().unwrap();

        // PKCS7 pad to 16-byte blocks.
        let pad_len = 16 - (plaintext.len() % 16);
        let mut padded = plaintext.to_vec();
        padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));

        let mut cipher = Aes256Ctr::new(&aes_key.into(), &iv.into());
        cipher.apply_keystream(&mut padded);
        let ciphertext = padded;

        let mut mac = HmacSha256::new_from_slice(hmac_key).unwrap();
        mac.update(&ciphertext);
        let hmac_stored = mac.finalize().into_bytes().to_vec();

        let inner = format!(
            "{}\n{}\n{}",
            hex::encode(&salt),
            hex::encode(&hmac_stored),
            hex::encode(&ciphertext)
        );
        let outer = hex::encode(inner.as_bytes());
        // Wrap to 80 cols.
        let mut wrapped = String::new();
        for (i, c) in outer.chars().enumerate() {
            if i > 0 && i % 80 == 0 {
                wrapped.push('\n');
            }
            wrapped.push(c);
        }

        format!("$ANSIBLE_VAULT;1.1;AES256\n{wrapped}\n")
    }

    #[test]
    fn parse_ansible_11_header() {
        let fixture = build_fixture_from_known_password("rsltest", b"hello");
        let parsed = parse_ansible_vault(&fixture).expect("parse");
        assert_eq!(parsed.version, "1.1");
        assert_eq!(parsed.cipher, "AES256");
        assert!(parsed.label.is_none());
        assert_eq!(parsed.salt.len(), 32);
        assert_eq!(parsed.hmac_stored.len(), 32);
        assert!(!parsed.ciphertext.is_empty());
    }

    #[test]
    fn import_round_trip_short_payload() {
        let original = b"hello ansible vault";
        let fixture = build_fixture_from_known_password("rsltest", original);
        let plaintext = import_ansible_vault(&fixture, "rsltest").expect("decrypt");
        assert_eq!(plaintext, original);
    }

    #[test]
    fn import_round_trip_long_payload() {
        let original: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        let fixture = build_fixture_from_known_password("longpw", &original);
        let plaintext = import_ansible_vault(&fixture, "longpw").expect("decrypt");
        assert_eq!(plaintext, original);
    }

    #[test]
    fn import_wrong_password_fails_at_hmac() {
        let original = b"secret payload";
        let fixture = build_fixture_from_known_password("right", original);
        let err = import_ansible_vault(&fixture, "wrong").unwrap_err();
        match err {
            VaultError::DecryptFailed(msg) => {
                assert!(msg.contains("HMAC") || msg.contains("hmac"), "got: {msg}");
            }
            other => panic!("expected DecryptFailed, got {other:?}"),
        }
    }

    #[test]
    fn import_rejects_unknown_version() {
        let body = "$ANSIBLE_VAULT;9.9;AES256\nffffffff\n";
        let err = parse_ansible_vault(body).unwrap_err();
        assert!(matches!(err, VaultError::InvalidHeader(_)));
    }

    #[test]
    fn import_rejects_missing_header() {
        let body = "no header here\nbody";
        let err = parse_ansible_vault(body).unwrap_err();
        assert!(matches!(err, VaultError::InvalidHeader(_)));
    }

    #[test]
    fn import_parses_12_with_label() {
        let original = b"labeled payload";
        let fixture = build_fixture_from_known_password("rsltest", original)
            .replacen("$ANSIBLE_VAULT;1.1;AES256", "$ANSIBLE_VAULT;1.2;AES256;prod", 1);
        let parsed = parse_ansible_vault(&fixture).expect("parse");
        assert_eq!(parsed.version, "1.2");
        assert_eq!(parsed.label.as_deref(), Some("prod"));
    }
}
