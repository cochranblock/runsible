// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! `$RUNSIBLE_VAULT` file envelope: header parse and emit.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

use crate::errors::{Result, VaultError};

/// Magic prefix for the vault header line.
pub const MAGIC: &str = "$RUNSIBLE_VAULT";
/// Protocol version field.
pub const VERSION: &str = "1";
/// Cipher suite field.
pub const CIPHER: &str = "CHACHA20-POLY1305";
/// Key type field.
pub const KEYTYPE: &str = "AGE";
/// Base64 line length for wrapping the body.
const LINE_LEN: usize = 76;

/// A parsed vault file envelope.
#[derive(Debug)]
pub struct VaultEnvelope {
    /// Number of age recipient stanzas in the encrypted payload.
    pub recipient_count: u32,
    /// The raw age binary payload (decoded from base64 body).
    pub body: Vec<u8>,
}

/// Parse a vault file (text) into its header metadata and binary payload.
///
/// Rejects files with CRLF line endings.
pub fn parse_envelope(raw: &str) -> Result<VaultEnvelope> {
    if raw.contains('\r') {
        return Err(VaultError::InvalidHeader(
            "CRLF line endings are not allowed in vault files".into(),
        ));
    }

    let (header_line, body_b64) = raw
        .split_once('\n')
        .ok_or_else(|| VaultError::InvalidHeader("missing newline after header".into()))?;

    // Parse the semicolon-separated header.
    let parts: Vec<&str> = header_line.splitn(6, ';').collect();
    // Expected: $RUNSIBLE_VAULT ; 1 ; CHACHA20-POLY1305 ; AGE ; <count>
    if parts.len() != 5 {
        return Err(VaultError::InvalidHeader(format!(
            "expected 5 header fields, got {}",
            parts.len()
        )));
    }
    if parts[0] != MAGIC {
        return Err(VaultError::InvalidHeader(format!(
            "bad magic: expected '{}', got '{}'",
            MAGIC, parts[0]
        )));
    }
    if parts[1] != VERSION {
        return Err(VaultError::InvalidHeader(format!(
            "unsupported version: '{}'",
            parts[1]
        )));
    }
    if parts[2] != CIPHER {
        return Err(VaultError::InvalidHeader(format!(
            "unsupported cipher: '{}'",
            parts[2]
        )));
    }
    if parts[3] != KEYTYPE {
        return Err(VaultError::InvalidHeader(format!(
            "unsupported key type: '{}'",
            parts[3]
        )));
    }
    let recipient_count: u32 = parts[4]
        .parse()
        .map_err(|_| VaultError::InvalidHeader(format!("invalid recipient count: '{}'", parts[4])))?;

    // Decode base64 body (strip any whitespace between wrapped lines).
    let b64_clean: String = body_b64.chars().filter(|c| !c.is_whitespace()).collect();
    let body = B64
        .decode(&b64_clean)
        .map_err(|e| VaultError::InvalidHeader(format!("base64 decode error: {e}")))?;

    Ok(VaultEnvelope {
        recipient_count,
        body,
    })
}

/// Emit a vault file string from an age binary payload.
pub fn emit_envelope(encrypted: &[u8], recipient_count: u32) -> String {
    let header = format!("{MAGIC};{VERSION};{CIPHER};{KEYTYPE};{recipient_count}");
    let b64 = B64.encode(encrypted);

    // Wrap at LINE_LEN columns.
    let wrapped = b64
        .as_bytes()
        .chunks(LINE_LEN)
        .map(|c| std::str::from_utf8(c).expect("base64 is ASCII"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{header}\n{wrapped}\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_parse_emit_roundtrip() {
        let body = b"hello age payload bytes go here 0123456789abcdef";
        let emitted = emit_envelope(body, 2);
        let parsed = parse_envelope(&emitted).expect("parse should succeed");
        assert_eq!(parsed.recipient_count, 2);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn envelope_rejects_crlf() {
        let with_crlf = "$RUNSIBLE_VAULT;1;CHACHA20-POLY1305;AGE;1\r\naGVsbG8=\n";
        let result = parse_envelope(with_crlf);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("CRLF"), "expected CRLF mention, got: {msg}");
    }

    #[test]
    fn envelope_rejects_bad_magic() {
        let bad = "$WRONGMAGIC;1;CHACHA20-POLY1305;AGE;1\naGVsbG8=\n";
        let result = parse_envelope(bad);
        assert!(result.is_err());
    }
}
