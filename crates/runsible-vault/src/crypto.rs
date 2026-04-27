// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! age encrypt/decrypt wrappers.

use std::io::{Read, Write};

use anyhow::Context;

/// Encrypt `plaintext` to the given `x25519` public key strings and return the age binary
/// payload.
///
/// `recipient_pubkeys` must be valid age bech32 public keys (e.g. `age1...`).
pub fn encrypt_bytes_to_keys(
    plaintext: &[u8],
    recipient_pubkeys: &[String],
) -> anyhow::Result<Vec<u8>> {
    let recipients: Vec<Box<dyn age::Recipient + Send>> = recipient_pubkeys
        .iter()
        .map(|s| {
            s.parse::<age::x25519::Recipient>()
                .map(|r| Box::new(r) as Box<dyn age::Recipient + Send>)
                .map_err(|e| anyhow::anyhow!("invalid recipient key '{}': {}", s, e))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    encrypt_bytes(plaintext, recipients)
}

/// Low-level: encrypt `plaintext` to already-constructed recipients.
pub fn encrypt_bytes(
    plaintext: &[u8],
    recipients: Vec<Box<dyn age::Recipient + Send>>,
) -> anyhow::Result<Vec<u8>> {
    let encryptor =
        age::Encryptor::with_recipients(recipients).context("no recipients provided")?;

    let mut ciphertext = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut ciphertext)
        .context("failed to initialise age encryptor")?;
    writer.write_all(plaintext).context("failed to write plaintext")?;
    writer.finish().context("failed to finalise encryption")?;

    Ok(ciphertext)
}

/// Decrypt `ciphertext` (age binary format) using the provided identities.
pub fn decrypt_bytes(
    ciphertext: &[u8],
    identities: &[Box<dyn age::Identity>],
) -> anyhow::Result<Vec<u8>> {
    let decryptor = match age::Decryptor::new(ciphertext)
        .context("failed to parse age ciphertext")?
    {
        age::Decryptor::Recipients(d) => d,
        age::Decryptor::Passphrase(_) => {
            anyhow::bail!("passphrase-encrypted files are not supported by runsible-vault")
        }
    };

    let mut plaintext = Vec::new();
    let mut reader = decryptor
        .decrypt(identities.iter().map(|id| id.as_ref() as &dyn age::Identity))
        .map_err(|e| anyhow::anyhow!("age decrypt error: {e}"))?;
    reader.read_to_end(&mut plaintext).context("failed to read decrypted stream")?;

    Ok(plaintext)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::keygen;

    /// Encrypt empty plaintext, decrypt, assert equal.
    #[test]
    fn crypto_encrypt_empty_plaintext() {
        let (identity, recipient) = keygen();
        let plaintext: &[u8] = b"";

        let recipients: Vec<Box<dyn age::Recipient + Send>> =
            vec![Box::new(recipient) as Box<dyn age::Recipient + Send>];
        let ciphertext = encrypt_bytes(plaintext, recipients).expect("encrypt empty");

        let identities: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity) as Box<dyn age::Identity>];
        let decrypted = decrypt_bytes(&ciphertext, &identities).expect("decrypt empty");

        assert_eq!(decrypted, plaintext);
        assert!(decrypted.is_empty());
    }

    /// Encrypt 100KB of pseudo-random plaintext, decrypt, assert equal.
    #[test]
    fn crypto_encrypt_100kb_plaintext() {
        let (identity, recipient) = keygen();

        // Build 100KB of varied bytes via a simple LCG (deterministic, no rand crate).
        let mut plaintext = Vec::with_capacity(100 * 1024);
        let mut state: u64 = 0xDEADBEEF;
        for _ in 0..(100 * 1024) {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            plaintext.push((state >> 33) as u8);
        }

        let recipients: Vec<Box<dyn age::Recipient + Send>> =
            vec![Box::new(recipient) as Box<dyn age::Recipient + Send>];
        let ciphertext = encrypt_bytes(&plaintext, recipients).expect("encrypt 100kb");

        let identities: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity) as Box<dyn age::Identity>];
        let decrypted = decrypt_bytes(&ciphertext, &identities).expect("decrypt 100kb");

        assert_eq!(decrypted.len(), 100 * 1024);
        assert_eq!(decrypted, plaintext);
    }

    /// Encrypt to two recipients; either one alone can decrypt.
    #[test]
    fn crypto_encrypt_multiple_recipients() {
        let (identity_a, recipient_a) = keygen();
        let (identity_b, recipient_b) = keygen();
        let plaintext = b"shared secret";

        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![
            Box::new(recipient_a) as Box<dyn age::Recipient + Send>,
            Box::new(recipient_b) as Box<dyn age::Recipient + Send>,
        ];
        let ciphertext = encrypt_bytes(plaintext, recipients).expect("encrypt 2-recip");

        let identities_a: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity_a) as Box<dyn age::Identity>];
        let decrypted_a = decrypt_bytes(&ciphertext, &identities_a).expect("decrypt with a");
        assert_eq!(decrypted_a, plaintext);

        let identities_b: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity_b) as Box<dyn age::Identity>];
        let decrypted_b = decrypt_bytes(&ciphertext, &identities_b).expect("decrypt with b");
        assert_eq!(decrypted_b, plaintext);
    }

    /// Decrypting with a totally unrelated key must fail.
    #[test]
    fn crypto_decrypt_with_wrong_key_errors() {
        let (_identity_a, recipient_a) = keygen();
        let (identity_b, _recipient_b) = keygen();
        let plaintext = b"top secret";

        let recipients: Vec<Box<dyn age::Recipient + Send>> =
            vec![Box::new(recipient_a) as Box<dyn age::Recipient + Send>];
        let ciphertext = encrypt_bytes(plaintext, recipients).expect("encrypt");

        let wrong_identities: Vec<Box<dyn age::Identity>> =
            vec![Box::new(identity_b) as Box<dyn age::Identity>];
        let result = decrypt_bytes(&ciphertext, &wrong_identities);
        assert!(result.is_err(), "decrypt with wrong key must error");
    }
}
