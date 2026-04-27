// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-vault — age-based file encryption for the runsible ecosystem.

pub mod crypto;
pub mod envelope;
pub mod errors;
pub mod keys;

pub use errors::{Result, VaultError};

// ---------------------------------------------------------------------------
// encrypt_string helper — produces a TOML inline-table snippet
// ---------------------------------------------------------------------------

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

/// Encrypt a string value and return a TOML inline-table snippet:
/// `{ vault = "v1", recipients = ["age1..."], ciphertext = "base64..." }`
///
/// Uses the provided recipient public keys; if `recipients` is empty the
/// caller's first key in the default key store is used.
pub fn encrypt_string(
    value: &str,
    recipient_pubkeys: &[String],
) -> anyhow::Result<String> {
    let ciphertext_bytes =
        crypto::encrypt_bytes_to_keys(value.as_bytes(), recipient_pubkeys)?;
    let ciphertext_b64 = B64.encode(&ciphertext_bytes);

    // Format the TOML snippet.
    let recipients_toml = recipient_pubkeys
        .iter()
        .map(|k| format!("\"{}\"", k))
        .collect::<Vec<_>>()
        .join(", ");
    let snippet = format!(
        r#"{{ vault = "v1", recipients = [{recipients_toml}], ciphertext = "{ciphertext_b64}" }}"#
    );
    Ok(snippet)
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt_bytes, encrypt_bytes};
    use crate::envelope::{emit_envelope, parse_envelope};
    use crate::keys::{keygen, KeyEntry, KeyStore};
    use age::secrecy::ExposeSecret as _;

    /// Encrypt "hello vault", decrypt, assert equality.
    #[test]
    fn roundtrip_encrypt_decrypt() {
        let (identity, recipient) = keygen();
        let plaintext = b"hello vault";

        let recipients: Vec<Box<dyn age::Recipient + Send>> =
            vec![Box::new(recipient) as Box<dyn age::Recipient + Send>];
        let ciphertext = encrypt_bytes(plaintext, recipients).expect("encrypt");

        let identities: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity) as Box<dyn age::Identity>];
        let decrypted = decrypt_bytes(&ciphertext, &identities).expect("decrypt");

        assert_eq!(decrypted, plaintext);
    }

    /// Emit an envelope, parse it, assert recipient_count and body match.
    #[test]
    fn envelope_parse_emit() {
        let body = b"arbitrary age binary payload 0xdeadbeef";
        let emitted = emit_envelope(body, 3);
        let parsed = parse_envelope(&emitted).expect("parse");
        assert_eq!(parsed.recipient_count, 3);
        assert_eq!(parsed.body, body);
    }

    /// Create a KeyStore with one entry, save, reload, assert same public key.
    #[test]
    fn keystore_roundtrip() {
        // Use a temp directory we own so chmod succeeds.
        let tmp_dir = tempfile::TempDir::new().expect("tempdir");
        let path = tmp_dir.path().join("keys.toml");

        let (identity, recipient) = keygen();
        let public_str = recipient.to_string();
        let private_str = identity.to_string().expose_secret().to_owned();

        let entry = KeyEntry {
            public: public_str.clone(),
            private: private_str,
            created: "2025-01-01T00:00:00Z".to_string(),
        };
        let mut store = KeyStore::default();
        store.add("default", entry);
        store.save(&path).expect("save");

        let loaded = KeyStore::load_or_default(&path).expect("load");
        assert_eq!(loaded.keys["default"].public, public_str);
    }

    /// `encrypt_string` returns valid TOML that parses.
    #[test]
    fn encrypt_string_snippet() {
        let (identity, recipient) = keygen();
        let public_str = recipient.to_string();

        let snippet = encrypt_string("secret", &[public_str.clone()]).expect("encrypt_string");

        // Should be valid inline TOML when wrapped as a value.
        let full_toml = format!("val = {snippet}\n");
        let parsed: toml::Value = toml::from_str(&full_toml).expect("toml parse");
        let table = parsed["val"].as_table().expect("val is a table");

        assert_eq!(table["vault"].as_str(), Some("v1"));
        assert_eq!(
            table["recipients"].as_array().unwrap()[0].as_str(),
            Some(public_str.as_str())
        );
        let ciphertext_b64 = table["ciphertext"].as_str().expect("ciphertext");
        assert!(!ciphertext_b64.is_empty());

        // Verify the ciphertext actually decrypts to "secret".
        let ct_bytes = B64.decode(ciphertext_b64).expect("b64 decode");
        let identities: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity) as Box<dyn age::Identity>];
        let plaintext = decrypt_bytes(&ct_bytes, &identities).expect("decrypt");
        assert_eq!(plaintext, b"secret");
    }

    /// `encrypt_string("hello", &[recipient])` returns valid TOML that parses
    /// to a table with `vault = "v1"`, `recipients`, and `ciphertext`.
    #[test]
    fn encrypt_string_returns_valid_v1_table() {
        let (_identity, recipient) = keygen();
        let public_str = recipient.to_string();

        let snippet = encrypt_string("hello", &[public_str.clone()]).expect("encrypt_string");

        let full_toml = format!("val = {snippet}\n");
        let parsed: toml::Value = toml::from_str(&full_toml).expect("toml parse");
        let table = parsed["val"].as_table().expect("val is a table");

        assert_eq!(table["vault"].as_str(), Some("v1"));
        let recipients = table["recipients"].as_array().expect("recipients array");
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].as_str(), Some(public_str.as_str()));
        assert!(table["ciphertext"].as_str().is_some(), "ciphertext field exists");
    }

    /// The ciphertext field must be base64 (only contains base64 alphabet + whitespace).
    #[test]
    fn encrypt_string_ciphertext_is_base64() {
        let (_identity, recipient) = keygen();
        let snippet = encrypt_string("hello", &[recipient.to_string()]).expect("encrypt_string");

        let full_toml = format!("val = {snippet}\n");
        let parsed: toml::Value = toml::from_str(&full_toml).expect("toml parse");
        let ct = parsed["val"]["ciphertext"]
            .as_str()
            .expect("ciphertext field");

        assert!(!ct.is_empty(), "ciphertext must not be empty");
        for c in ct.chars() {
            // Match: ^[A-Za-z0-9+/=\s]+$
            assert!(
                c.is_ascii_alphanumeric()
                    || c == '+'
                    || c == '/'
                    || c == '='
                    || c.is_ascii_whitespace(),
                "non-base64 char in ciphertext: {c:?}"
            );
        }

        // And it must round-trip through base64 decode (sanity check).
        assert!(
            B64.decode(ct).is_ok(),
            "ciphertext must decode as base64"
        );
    }
}
