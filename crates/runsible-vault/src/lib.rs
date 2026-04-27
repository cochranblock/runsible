// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-vault — age-based file encryption for the runsible ecosystem.

pub mod ansible_import;
pub mod crypto;
pub mod envelope;
pub mod errors;
pub mod keys;
pub mod recipients;

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
// f30 — TRIPLE SIMS smoke gate
// ---------------------------------------------------------------------------

/// Smoke gate: exercise the public API end-to-end. Generate an age keypair,
/// encrypt a fixed payload to it, decrypt and verify the round-trip, then
/// emit a vault envelope around the ciphertext and parse it back. Returns
/// 0 on success or a non-zero stage code on failure. Used by the
/// runsible-vault-test binary's TRIPLE SIMS gate.
pub fn f30() -> i32 {
    use crate::ansible_import::{decrypt_ansible_vault, parse_ansible_vault};
    use crate::crypto::{decrypt_bytes, encrypt_bytes};
    use crate::envelope::{emit_envelope, parse_envelope};
    use crate::keys::{keygen, KeyEntry, KeyStore};
    use crate::recipients::{add_recipient, list_recipients, remove_recipient, rekey_to};
    use age::secrecy::ExposeSecret as _;

    const PAYLOAD: &[u8] = b"runsible-vault f30 payload";

    // Stage 1: generate a fresh age keypair.
    let (identity, recipient) = keygen();
    let recipient_pub = recipient.to_string();

    // Stage 2: encrypt the payload to the recipient.
    let recipients: Vec<Box<dyn age::Recipient + Send>> =
        vec![Box::new(recipient.clone()) as Box<dyn age::Recipient + Send>];
    let ciphertext = match encrypt_bytes(PAYLOAD, recipients) {
        Ok(c) => c,
        Err(_) => return 1,
    };

    // Stage 3: decrypt the ciphertext using the matching identity.
    let identities: Vec<Box<dyn age::Identity>> =
        vec![Box::new(identity.clone()) as Box<dyn age::Identity>];
    let decrypted = match decrypt_bytes(&ciphertext, &identities) {
        Ok(d) => d,
        Err(_) => return 2,
    };
    if decrypted != PAYLOAD {
        return 3;
    }

    // Stage 4: envelope round-trip — emit then parse, body must match ciphertext.
    let envelope = emit_envelope(&ciphertext, 1);
    let parsed = match parse_envelope(&envelope) {
        Ok(p) => p,
        Err(_) => return 4,
    };
    if parsed.recipient_count != 1 {
        return 5;
    }
    if parsed.body != ciphertext {
        return 6;
    }

    // Stage 5: list_recipients must find exactly one X25519 stanza
    // (and skip age's grease padding).
    let listed = match list_recipients(&envelope) {
        Ok(v) => v,
        Err(_) => return 7,
    };
    if listed.len() != 1 || !listed[0].starts_with("X25519 ") {
        return 8;
    }

    // Stage 6: rekey_to a different recipient set proves the new recipient
    // can decrypt and the old one cannot. Build a keystore with `identity`.
    let mut store = KeyStore::default();
    store.add(
        "f30",
        KeyEntry {
            public: recipient_pub.clone(),
            private: identity.to_string().expose_secret().to_owned(),
            created: "f30".to_string(),
        },
    );

    let (new_id, new_recipient) = keygen();
    let new_pub = new_recipient.to_string();

    // add_recipient: existing = [recipient_pub], new = new_pub.
    let env_after_add = match add_recipient(&envelope, &new_pub, &[recipient_pub.clone()], &store) {
        Ok(s) => s,
        Err(_) => return 9,
    };

    // The new recipient must be able to decrypt.
    let env_after_add_parsed = match parse_envelope(&env_after_add) {
        Ok(p) => p,
        Err(_) => return 10,
    };
    let new_identities: Vec<Box<dyn age::Identity>> =
        vec![Box::new(new_id.clone()) as Box<dyn age::Identity>];
    let dec_new = match decrypt_bytes(&env_after_add_parsed.body, &new_identities) {
        Ok(d) => d,
        Err(_) => return 11,
    };
    if dec_new != PAYLOAD {
        return 12;
    }

    // Stage 7: remove_recipient revokes the new recipient. Build store with new_id
    // (old recipient may have been dropped, so use new_id as the proving identity).
    let mut store_new = KeyStore::default();
    store_new.add(
        "f30new",
        KeyEntry {
            public: new_pub.clone(),
            private: new_id.to_string().expose_secret().to_owned(),
            created: "f30".to_string(),
        },
    );
    let env_after_remove = match remove_recipient(
        &env_after_add,
        &new_pub,
        &[recipient_pub.clone(), new_pub.clone()],
        &store_new,
    ) {
        Ok(s) => s,
        Err(_) => return 13,
    };
    let env_after_remove_parsed = match parse_envelope(&env_after_remove) {
        Ok(p) => p,
        Err(_) => return 14,
    };
    // new_id must NOT be able to decrypt the rekeyed file.
    if decrypt_bytes(&env_after_remove_parsed.body, &new_identities).is_ok() {
        return 15;
    }
    // Old identity (in `store`) MUST still decrypt.
    let old_identities: Vec<Box<dyn age::Identity>> =
        vec![Box::new(identity) as Box<dyn age::Identity>];
    let dec_old = match decrypt_bytes(&env_after_remove_parsed.body, &old_identities) {
        Ok(d) => d,
        Err(_) => return 16,
    };
    if dec_old != PAYLOAD {
        return 17;
    }

    // Stage 8: rekey_to with empty recipients must error.
    if rekey_to(&envelope, &[], &store).is_ok() {
        return 18;
    }

    // Stage 9: ansible-vault import round-trip. Build a deterministic
    // $ANSIBLE_VAULT;1.1 fixture in-process, then parse + decrypt it.
    let pw = "rsl-f30-pw";
    let pt = b"imported by f30";
    let fixture = build_ansible_vault_fixture(pw, pt);
    let parsed_av = match parse_ansible_vault(&fixture) {
        Ok(p) => p,
        Err(_) => return 19,
    };
    if parsed_av.version != "1.1" || parsed_av.cipher != "AES256" {
        return 20;
    }
    let decrypted_legacy = match decrypt_ansible_vault(&parsed_av, pw) {
        Ok(d) => d,
        Err(_) => return 21,
    };
    if decrypted_legacy != pt {
        return 22;
    }
    // Wrong-password path must fail at HMAC check.
    if decrypt_ansible_vault(&parsed_av, "wrong").is_ok() {
        return 23;
    }

    0
}

/// Build a deterministic `$ANSIBLE_VAULT;1.1;AES256` fixture for use in f30.
/// Uses the same KDF / cipher / MAC pattern as Ansible itself.
#[doc(hidden)]
pub fn build_ansible_vault_fixture(password: &str, plaintext: &[u8]) -> String {
    use aes::Aes256;
    use ctr::cipher::{KeyIvInit, StreamCipher};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type Aes256Ctr = ctr::Ctr64BE<Aes256>;
    type HmacSha256 = Hmac<Sha256>;

    let salt: [u8; 32] = [0x42; 32];
    let mut derived = [0u8; 80];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 10_000, &mut derived);
    let aes_key: [u8; 32] = derived[0..32].try_into().unwrap();
    let hmac_key = &derived[32..64];
    let iv: [u8; 16] = derived[64..80].try_into().unwrap();

    let pad_len = 16 - (plaintext.len() % 16);
    let mut padded = plaintext.to_vec();
    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    let mut cipher = Aes256Ctr::new(&aes_key.into(), &iv.into());
    cipher.apply_keystream(&mut padded);
    let ct = padded;

    let mut mac = HmacSha256::new_from_slice(hmac_key).unwrap();
    mac.update(&ct);
    let stored_hmac = mac.finalize().into_bytes().to_vec();

    let inner = format!(
        "{}\n{}\n{}",
        hex::encode(&salt),
        hex::encode(&stored_hmac),
        hex::encode(&ct)
    );
    let outer = hex::encode(inner.as_bytes());
    let mut wrapped = String::new();
    for (i, c) in outer.chars().enumerate() {
        if i > 0 && i % 80 == 0 {
            wrapped.push('\n');
        }
        wrapped.push(c);
    }
    format!("$ANSIBLE_VAULT;1.1;AES256\n{wrapped}\n")
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
