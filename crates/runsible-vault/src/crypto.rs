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
