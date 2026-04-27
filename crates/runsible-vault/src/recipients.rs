//! Manage recipients on an existing runsible vault file.
//!
//! Conceptual model: the body of a runsible vault file is encrypted under a
//! single per-file Data Encryption Key (DEK) that age wraps for each recipient
//! in the header stanzas. "Adding" or "removing" a recipient ideally means
//! splicing a wrapped-DEK stanza into the header, leaving body bytes untouched.
//!
//! age does NOT expose stanza splicing as a public API — its Encryptor
//! always emits a fresh DEK for the entire payload. Furthermore, age's
//! wrapped-DEK header reveals the PROTECTED short-key derivation, NOT the
//! recipient's full public key. So "current recipients" cannot be recovered
//! from the file — we only know the count.
//!
//! Pragmatic M1 implementation: `rekey_to(file, new_recipients, keystore)` is
//! the load-bearing primitive. It decrypts using a known-good identity from
//! the keystore (proving access), then re-encrypts under the supplied full
//! recipient set. Callers (the CLI's `recipients add` / `recipients remove`)
//! are responsible for computing the new full set:
//!
//!   - `add <file> --recipient <new> [--recipient <existing>...]`
//!   - `remove <file> --keep <existing>...`
//!
//! In both cases the new envelope's body bytes change (re-encryption under a
//! fresh DEK). The old plan promised bit-identical body retention; that's a
//! deferred-to-M2 promise pending an age fork that exposes header splicing.

use crate::crypto::{decrypt_bytes, encrypt_bytes_to_keys};
use crate::envelope::{emit_envelope, parse_envelope};
use crate::errors::{Result, VaultError};
use crate::keys::KeyStore;

/// List the recipient algorithm + identifier strings appearing in the file's
/// age header. NOTE: for X25519 recipients the identifier is age's
/// header-internal short-key share, NOT the original `age1...` bech32 public
/// key. So this output is only useful for counting and for distinguishing
/// X25519 vs ssh-ed25519 vs ssh-rsa recipients — it cannot be fed back into
/// `rekey_to` as a recipient list.
pub fn list_recipients(envelope_text: &str) -> Result<Vec<String>> {
    let env = parse_envelope(envelope_text)?;
    Ok(extract_recipients(&env.body))
}

/// Re-encrypt the file under a new full recipient set. The decryption proof
/// comes from `keystore` (must hold a private key that's a current recipient).
/// Returns the new envelope text.
pub fn rekey_to(
    envelope_text: &str,
    new_recipients: &[String],
    keystore: &KeyStore,
) -> Result<String> {
    if new_recipients.is_empty() {
        return Err(VaultError::RecipientParse(
            "rekey_to: new_recipients must not be empty".into(),
        ));
    }
    let env = parse_envelope(envelope_text)?;
    let identities = keystore.private_identities();
    if identities.is_empty() {
        return Err(VaultError::NoPrivateKey);
    }
    let plaintext = decrypt_bytes(&env.body, &identities)
        .map_err(|e| VaultError::DecryptFailed(format!("rekey decrypt: {e}")))?;
    let new_body = encrypt_bytes_to_keys(&plaintext, new_recipients)
        .map_err(|e| VaultError::DecryptFailed(format!("rekey encrypt: {e}")))?;
    let count = new_recipients.len() as u32;
    Ok(emit_envelope(&new_body, count))
}

/// Convenience: rekey adding one new recipient to an explicit existing set.
/// `existing_recipients` should be the user's current recipient pubkeys
/// (typically read from a project recipients file). Returns Err if `new`
/// would duplicate an existing entry (caller can ignore that condition).
pub fn add_recipient(
    envelope_text: &str,
    new_recipient: &str,
    existing_recipients: &[String],
    keystore: &KeyStore,
) -> Result<String> {
    let mut combined: Vec<String> = existing_recipients.to_vec();
    if !combined.iter().any(|r| r == new_recipient) {
        combined.push(new_recipient.to_string());
    }
    rekey_to(envelope_text, &combined, keystore)
}

/// Convenience: rekey dropping one recipient from an explicit existing set.
/// `existing_recipients` is the full current recipient set. Errors if
/// `drop` is not present, or if removal would empty the recipient set.
pub fn remove_recipient(
    envelope_text: &str,
    drop_recipient: &str,
    existing_recipients: &[String],
    keystore: &KeyStore,
) -> Result<String> {
    let kept: Vec<String> = existing_recipients
        .iter()
        .filter(|r| r.as_str() != drop_recipient)
        .cloned()
        .collect();
    if kept.len() == existing_recipients.len() {
        return Err(VaultError::RecipientParse(format!(
            "recipient {drop_recipient} is not in the supplied existing set"
        )));
    }
    if kept.is_empty() {
        return Err(VaultError::RecipientParse(
            "removing this recipient would leave the file with zero recipients (refusing)".into(),
        ));
    }
    rekey_to(envelope_text, &kept, keystore)
}

/// Walk the age payload and pull out recipient algorithm/identifier pairs from
/// the header. Bounded by the `\n---` ASCII header-end marker so binary body
/// bytes can't masquerade as `-> ` lines.
fn extract_recipients(age_payload: &[u8]) -> Vec<String> {
    let header_end = find_header_end(age_payload).unwrap_or(age_payload.len());
    let header = String::from_utf8_lossy(&age_payload[..header_end]);
    let mut out = Vec::new();
    for line in header.lines() {
        let Some(rest) = line.strip_prefix("-> ") else { continue };
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        // Filter known recipient algorithms; skip age's "grease" stanza
        // (random anti-stanza-injection padding) and anything else exotic.
        let algo = parts[0];
        let known = matches!(
            algo,
            "X25519" | "ssh-ed25519" | "ssh-rsa" | "scrypt"
        );
        if !known {
            continue;
        }
        out.push(format!("{} {}", algo, parts[1]));
    }
    out
}

/// Find the byte offset of the first `\n---` (header-end marker) in the age
/// payload. The header is pure ASCII; everything after is binary body.
fn find_header_end(payload: &[u8]) -> Option<usize> {
    payload.windows(4).position(|w| w == b"\n---")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encrypt_bytes;
    use crate::envelope::emit_envelope;
    use crate::keys::{keygen, KeyEntry, KeyStore};
    use age::secrecy::ExposeSecret as _;

    fn store_with(identity: &age::x25519::Identity) -> KeyStore {
        let mut store = KeyStore::default();
        store.add(
            "default",
            KeyEntry {
                public: identity.to_public().to_string(),
                private: identity.to_string().expose_secret().to_owned(),
                created: "2026-04-27T00:00:00Z".to_string(),
            },
        );
        store
    }

    fn make_envelope(recipients: &[age::x25519::Recipient]) -> String {
        let r: Vec<Box<dyn age::Recipient + Send>> = recipients
            .iter()
            .map(|r| Box::new(r.clone()) as Box<dyn age::Recipient + Send>)
            .collect();
        let body = encrypt_bytes(b"the original secret", r).expect("encrypt");
        emit_envelope(&body, recipients.len() as u32)
    }

    #[test]
    fn list_recipients_finds_x25519_stanzas() {
        let (_id1, r1) = keygen();
        let (_id2, r2) = keygen();
        let env = make_envelope(&[r1, r2]);
        let listed = list_recipients(&env).expect("list");
        // Each X25519 stanza yields one entry; grease/HMAC are filtered out.
        assert_eq!(listed.len(), 2);
        for entry in &listed {
            assert!(entry.starts_with("X25519 "), "got: {entry}");
        }
    }

    #[test]
    fn rekey_to_makes_new_recipient_decrypt_and_old_decrypt_too() {
        let (id1, r1) = keygen();
        let r1_pub = r1.to_string();
        let env = make_envelope(&[r1]);

        let (id2, r2) = keygen();
        let r2_pub = r2.to_string();

        let new_env = rekey_to(&env, &[r1_pub.clone(), r2_pub.clone()], &store_with(&id1))
            .expect("rekey");

        // Both id1 and id2 must be able to decrypt.
        let env2 = parse_envelope(&new_env).expect("parse");
        let identities2: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id2) as Box<dyn age::Identity>];
        let p_new = decrypt_bytes(&env2.body, &identities2).expect("new recipient decrypt");
        assert_eq!(p_new, b"the original secret");
        let identities1: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id1) as Box<dyn age::Identity>];
        let p_old = decrypt_bytes(&env2.body, &identities1).expect("old recipient decrypt");
        assert_eq!(p_old, b"the original secret");
    }

    #[test]
    fn rekey_to_revokes_dropped_recipient() {
        let (id1, r1) = keygen();
        let (id2, r2) = keygen();
        let env = make_envelope(&[r1.clone(), r2]);

        // Rekey using id1's keystore, dropping r2 from the recipient set.
        let new_env = rekey_to(&env, &[r1.to_string()], &store_with(&id1)).expect("rekey");

        let env2 = parse_envelope(&new_env).expect("parse");
        let identities2: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id2) as Box<dyn age::Identity>];
        assert!(
            decrypt_bytes(&env2.body, &identities2).is_err(),
            "dropped recipient must not be able to decrypt"
        );
        let identities1: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id1) as Box<dyn age::Identity>];
        let p = decrypt_bytes(&env2.body, &identities1).expect("kept recipient decrypts");
        assert_eq!(p, b"the original secret");
    }

    #[test]
    fn rekey_to_empty_recipient_set_errors() {
        let (id1, r1) = keygen();
        let env = make_envelope(&[r1]);
        let r = rekey_to(&env, &[], &store_with(&id1));
        assert!(r.is_err());
    }

    #[test]
    fn rekey_to_without_known_identity_errors() {
        let (_id1, r1) = keygen();
        let env = make_envelope(&[r1]);
        // Use a different identity than the file's recipient — decrypt fails.
        let (id_other, _) = keygen();
        let r = rekey_to(&env, &["age1unused".into()], &store_with(&id_other));
        assert!(r.is_err());
    }

    #[test]
    fn add_recipient_appends_to_existing_set() {
        let (id1, r1) = keygen();
        let r1_pub = r1.to_string();
        let env = make_envelope(&[r1]);

        let (id2, r2) = keygen();
        let new_env = add_recipient(&env, &r2.to_string(), &[r1_pub.clone()], &store_with(&id1))
            .expect("add");

        let env2 = parse_envelope(&new_env).expect("parse");
        let identities2: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id2) as Box<dyn age::Identity>];
        assert_eq!(
            decrypt_bytes(&env2.body, &identities2).expect("new recipient decrypts"),
            b"the original secret"
        );
    }

    #[test]
    fn add_recipient_idempotent_when_already_present() {
        let (id1, r1) = keygen();
        let r1_pub = r1.to_string();
        let env = make_envelope(&[r1]);

        let new_env = add_recipient(&env, &r1_pub, &[r1_pub.clone()], &store_with(&id1))
            .expect("add same");
        let listed = list_recipients(&new_env).expect("list");
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn remove_recipient_drops_from_existing_set() {
        let (id1, r1) = keygen();
        let r1_pub = r1.to_string();
        let (id2, r2) = keygen();
        let r2_pub = r2.to_string();
        let env = make_envelope(&[r1, r2]);

        let new_env = remove_recipient(
            &env,
            &r2_pub,
            &[r1_pub.clone(), r2_pub.clone()],
            &store_with(&id1),
        )
        .expect("remove");

        let env2 = parse_envelope(&new_env).expect("parse");
        let identities2: Vec<Box<dyn age::Identity>> =
            vec![Box::new(id2) as Box<dyn age::Identity>];
        assert!(
            decrypt_bytes(&env2.body, &identities2).is_err(),
            "removed recipient must not decrypt"
        );
    }

    #[test]
    fn remove_only_recipient_refuses() {
        let (id1, r1) = keygen();
        let r1_pub = r1.to_string();
        let env = make_envelope(&[r1]);
        let r = remove_recipient(&env, &r1_pub, &[r1_pub.clone()], &store_with(&id1));
        assert!(r.is_err(), "removing the only recipient must error");
    }

    #[test]
    fn remove_nonexistent_recipient_errors() {
        let (id1, r1) = keygen();
        let env = make_envelope(&[r1.clone()]);
        let r = remove_recipient(
            &env,
            "age1totallyunknown",
            &[r1.to_string()],
            &store_with(&id1),
        );
        assert!(r.is_err());
    }
}
