// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! Key store: `~/.runsible/keys.toml` read/write.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::errors::Result;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single named key pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntry {
    pub public: String,
    pub private: String,
    #[serde(default = "default_created")]
    pub created: String,
}

/// Return the default path for the runsible key store: `~/.runsible/keys.toml`.
pub fn default_keys_path() -> PathBuf {
    dirs_next_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".runsible")
        .join("keys.toml")
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Return a naive UTC timestamp string (ISO 8601, seconds precision).
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fmt_iso8601(secs)
}

fn fmt_iso8601(s: u64) -> String {
    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let days_since_epoch = s / 86400;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn default_created() -> String {
    now_iso8601()
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Gregorian calendar approximation. Good until 2100.
    let year_start = 1970u64;
    let mut year = year_start;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = [31u64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ---------------------------------------------------------------------------
// KeyStore
// ---------------------------------------------------------------------------

/// The deserialized contents of `~/.runsible/keys.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyStore {
    #[serde(default)]
    pub keys: IndexMap<String, KeyEntry>,
}

impl KeyStore {
    /// Load an existing key store from `path`, or return an empty store if the
    /// file does not exist.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        let store: KeyStoreFile = toml::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(store.into())
    }

    /// Persist the key store to `path` with secure permissions.
    ///
    /// The parent directory is created with mode 0700, the file with mode 0600.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
            set_dir_mode_0700(dir)?;
        }

        let on_disk = KeyStoreFile::from(self.clone());
        let toml_str = toml::to_string_pretty(&on_disk)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // Write atomically-ish via a temp file, then rename.
        let tmp_path = path.with_extension("toml.tmp");
        {
            let mut f = create_secure_file(&tmp_path)?;
            f.write_all(toml_str.as_bytes())?;
            f.flush()?;
        }
        fs::rename(&tmp_path, path)?;
        set_file_mode_0600(path)?;

        Ok(())
    }

    /// Insert or replace a key entry under `label`.
    pub fn add(&mut self, label: &str, entry: KeyEntry) {
        self.keys.insert(label.to_owned(), entry);
    }

    /// Return a list of age identities for all private keys in the store.
    pub fn private_identities(&self) -> Vec<Box<dyn age::Identity>> {
        self.keys
            .values()
            .filter_map(|entry| {
                entry
                    .private
                    .parse::<age::x25519::Identity>()
                    .ok()
                    .map(|id| Box::new(id) as Box<dyn age::Identity>)
            })
            .collect()
    }

    /// Return the public key string of the first key in the store, if any.
    pub fn first_public_key(&self) -> Option<String> {
        self.keys.values().next().map(|e| e.public.clone())
    }
}

// ---------------------------------------------------------------------------
// Key generation
// ---------------------------------------------------------------------------

/// Generate a new age X25519 key pair.
pub fn keygen() -> (age::x25519::Identity, age::x25519::Recipient) {
    let identity = age::x25519::Identity::generate();
    let recipient = identity.to_public();
    (identity, recipient)
}

// ---------------------------------------------------------------------------
// TOML on-disk shape
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct KeyStoreFile {
    #[serde(default)]
    keys: IndexMap<String, KeyEntry>,
}

impl From<KeyStoreFile> for KeyStore {
    fn from(f: KeyStoreFile) -> Self {
        KeyStore { keys: f.keys }
    }
}

impl From<KeyStore> for KeyStoreFile {
    fn from(s: KeyStore) -> Self {
        KeyStoreFile { keys: s.keys }
    }
}

// ---------------------------------------------------------------------------
// Platform file-permission helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn create_secure_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?)
}

#[cfg(not(unix))]
fn create_secure_file(path: &Path) -> Result<fs::File> {
    Ok(fs::File::create(path)?)
}

#[cfg(unix)]
fn set_dir_mode_0700(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o700);
    fs::set_permissions(dir, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_mode_0700(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode_0600(_path: &Path) -> Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use age::secrecy::ExposeSecret as _;

    #[test]
    fn keystore_roundtrip() {
        // Use a temp *directory* we own so chmod 0700 on it succeeds.
        let tmp_dir = tempfile::TempDir::new().expect("tempdir");
        let path = tmp_dir.path().join("keys.toml");

        let (identity, recipient) = keygen();
        let entry = KeyEntry {
            public: recipient.to_string(),
            private: identity.to_string().expose_secret().to_owned(),
            created: default_created(),
        };

        let mut store = KeyStore::default();
        store.add("test", entry.clone());
        store.save(&path).expect("save");

        let loaded = KeyStore::load_or_default(&path).expect("load");
        assert_eq!(
            loaded.keys["test"].public,
            entry.public,
            "public key must survive roundtrip"
        );
    }

    /// With two entries, first_public_key returns the first one inserted.
    #[test]
    fn keystore_first_public_key_returns_first_entry() {
        let (id_a, rec_a) = keygen();
        let (id_b, rec_b) = keygen();

        let entry_a = KeyEntry {
            public: rec_a.to_string(),
            private: id_a.to_string().expose_secret().to_owned(),
            created: "2025-01-01T00:00:00Z".to_string(),
        };
        let entry_b = KeyEntry {
            public: rec_b.to_string(),
            private: id_b.to_string().expose_secret().to_owned(),
            created: "2025-01-02T00:00:00Z".to_string(),
        };

        let mut store = KeyStore::default();
        store.add("alpha", entry_a.clone());
        store.add("bravo", entry_b);

        assert_eq!(
            store.first_public_key().as_deref(),
            Some(entry_a.public.as_str()),
            "first_public_key must return the first inserted entry's pubkey"
        );
    }

    /// KeyStore.add overwrites an existing label (insert semantics on the same key).
    #[test]
    fn keystore_add_overwrites_existing_label() {
        let (id_a, rec_a) = keygen();
        let (id_b, rec_b) = keygen();

        let entry_a = KeyEntry {
            public: rec_a.to_string(),
            private: id_a.to_string().expose_secret().to_owned(),
            created: "2025-01-01T00:00:00Z".to_string(),
        };
        let entry_b = KeyEntry {
            public: rec_b.to_string(),
            private: id_b.to_string().expose_secret().to_owned(),
            created: "2025-01-02T00:00:00Z".to_string(),
        };

        let mut store = KeyStore::default();
        store.add("default", entry_a);
        store.add("default", entry_b.clone());

        assert_eq!(store.keys.len(), 1, "label collision must overwrite, not append");
        assert_eq!(store.keys["default"].public, entry_b.public);
    }
}
