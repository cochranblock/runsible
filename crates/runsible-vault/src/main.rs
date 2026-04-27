// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-vault CLI.

use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

use age::secrecy::ExposeSecret as _;
use anyhow::Context;
use clap::{Parser, Subcommand};

use runsible_vault::{
    crypto::{decrypt_bytes, encrypt_bytes_to_keys},
    envelope::{emit_envelope, parse_envelope},
    keys::{default_keys_path, keygen, KeyEntry, KeyStore},
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "runsible-vault", about = "age-based file encryption for runsible", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new age X25519 key pair.
    Keygen {
        /// Label to store the key under in keys.toml.
        #[arg(long, default_value = "default")]
        label: String,
        /// Path to keys.toml (default: ~/.runsible/keys.toml).
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Encrypt a file in-place (writes <file>.vault).
    Encrypt {
        /// Path to the plaintext file.
        file: PathBuf,
        /// Recipient public key(s). Repeatable. Defaults to first key in keys.toml.
        #[arg(long = "recipient", short = 'r')]
        recipients: Vec<String>,
    },

    /// Decrypt a vault file (writes the plaintext without the .vault extension).
    Decrypt {
        /// Path to the vault file (must end in .vault).
        file: PathBuf,
    },

    /// Manage recipients.
    Recipients {
        #[command(subcommand)]
        cmd: RecipientsCmd,
    },

    /// Encrypt a string from stdin (or --value) and emit a TOML inline snippet.
    EncryptString {
        /// Value to encrypt. If omitted, read from stdin.
        #[arg(long)]
        value: Option<String>,
        /// Recipient public key(s). Repeatable. Defaults to first key in keys.toml.
        #[arg(long = "recipient", short = 'r')]
        recipients: Vec<String>,
    },

    /// Import a legacy `$ANSIBLE_VAULT;1.1`/`;1.2` file: decrypt with a
    /// password, then re-encrypt under runsible recipients (writes
    /// <file>.vault and leaves the original alone).
    ImportAnsible {
        /// Path to the legacy ansible-vault file.
        file: PathBuf,
        /// Read the legacy password from this file (newline-trimmed).
        /// Mutually exclusive with `--password`.
        #[arg(long = "password-file")]
        password_file: Option<PathBuf>,
        /// Pass the legacy password literally on the command line. NOT
        /// recommended (visible in shell history); use --password-file.
        #[arg(long)]
        password: Option<String>,
        /// Recipient public key(s) for the re-encrypted output. Repeatable.
        #[arg(long = "recipient", short = 'r')]
        recipients: Vec<String>,
        /// Write to this path instead of <file>.vault.
        #[arg(long = "out", short = 'o')]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum RecipientsCmd {
    /// List recipients recorded in the vault file header (algorithm + short
    /// header-share, NOT the original public keys — age doesn't expose those).
    List {
        /// Path to the vault file.
        file: PathBuf,
    },
    /// Add a recipient to an existing vault file. Re-encrypts the body under
    /// `existing` ∪ `recipient`. Caller must list ALL existing recipients via
    /// repeated `--existing` flags so previously-authorized parties stay
    /// authorized; recipients not listed will be silently dropped.
    Add {
        file: PathBuf,
        /// New recipient public key to authorize.
        #[arg(long)]
        recipient: String,
        /// Current recipient set (repeatable). Required so previously-
        /// authorized parties stay authorized after rekey.
        #[arg(long = "existing")]
        existing: Vec<String>,
    },
    /// Remove a recipient. Re-encrypts the body under `existing` minus
    /// `recipient`. Same explicit-existing-set requirement as `add`.
    Remove {
        file: PathBuf,
        #[arg(long)]
        recipient: String,
        #[arg(long = "existing")]
        existing: Vec<String>,
    },
    /// Re-key the file to a fully specified new recipient set. Use this when
    /// you want to set the recipient list explicitly (replacing whatever was
    /// there).
    Rekey {
        file: PathBuf,
        #[arg(long = "recipient", short = 'r')]
        recipients: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Keygen { label, out } => cmd_keygen(&label, out.as_deref()),
        Command::Encrypt { file, recipients } => cmd_encrypt(&file, &recipients),
        Command::Decrypt { file } => cmd_decrypt(&file),
        Command::Recipients { cmd } => match cmd {
            RecipientsCmd::List { file } => cmd_recipients_list(&file),
            RecipientsCmd::Add { file, recipient, existing } => {
                cmd_recipients_add(&file, &recipient, &existing)
            }
            RecipientsCmd::Remove { file, recipient, existing } => {
                cmd_recipients_remove(&file, &recipient, &existing)
            }
            RecipientsCmd::Rekey { file, recipients } => cmd_recipients_rekey(&file, &recipients),
        },
        Command::EncryptString { value, recipients } => cmd_encrypt_string(value, &recipients),
        Command::ImportAnsible {
            file,
            password_file,
            password,
            recipients,
            out,
        } => cmd_import_ansible(&file, password_file.as_deref(), password.as_deref(), &recipients, out.as_deref()),
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

/// `keygen [--label <name>] [--out <path>]`
fn cmd_keygen(label: &str, out: Option<&Path>) -> anyhow::Result<()> {
    let keys_path = out.map(PathBuf::from).unwrap_or_else(default_keys_path);

    let (identity, recipient) = keygen();
    let public_str = recipient.to_string();
    let private_str = identity.to_string().expose_secret().to_owned();

    let mut store = KeyStore::load_or_default(&keys_path)
        .context("loading existing key store")?;

    let entry = KeyEntry {
        public: public_str.clone(),
        private: private_str,
        created: runsible_vault::keys::now_iso8601(),
    };
    store.add(label, entry);
    store.save(&keys_path)
        .with_context(|| format!("saving key store to {}", keys_path.display()))?;

    println!("{public_str}");
    Ok(())
}

/// `encrypt <file> [--recipient <pubkey>]...`
fn cmd_encrypt(file: &Path, cli_recipients: &[String]) -> anyhow::Result<()> {
    let plaintext = std::fs::read(file)
        .with_context(|| format!("reading {}", file.display()))?;

    let recipients = resolve_recipients(cli_recipients)?;
    let recipient_count = recipients.len() as u32;
    let ciphertext = encrypt_bytes_to_keys(&plaintext, &recipients)
        .context("encrypting file")?;

    let envelope = emit_envelope(&ciphertext, recipient_count);

    // Append ".vault" suffix (e.g. "secrets.toml" → "secrets.toml.vault").
    let out_path = {
        let mut p = file.as_os_str().to_owned();
        p.push(".vault");
        PathBuf::from(p)
    };

    std::fs::write(&out_path, envelope.as_bytes())
        .with_context(|| format!("writing {}", out_path.display()))?;

    println!("encrypted → {}", out_path.display());
    Ok(())
}

/// `decrypt <file.vault>`
fn cmd_decrypt(file: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;

    let envelope = parse_envelope(&raw)
        .map_err(|e| anyhow::anyhow!("vault parse error: {e}"))?;

    let keys_path = default_keys_path();
    let store = KeyStore::load_or_default(&keys_path)
        .context("loading key store")?;
    let identities = store.private_identities();
    if identities.is_empty() {
        anyhow::bail!("no private keys in key store at {}", keys_path.display());
    }

    let plaintext = decrypt_bytes(&envelope.body, &identities)
        .context("decrypting vault file")?;

    // Strip ".vault" suffix to get output path.
    let out_path = file
        .to_str()
        .and_then(|s| s.strip_suffix(".vault"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("file must have .vault extension"))?;

    std::fs::write(&out_path, &plaintext)
        .with_context(|| format!("writing {}", out_path.display()))?;

    println!("decrypted → {}", out_path.display());
    Ok(())
}

/// `recipients list <file.vault>`
fn cmd_recipients_list(file: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let envelope = parse_envelope(&raw)
        .map_err(|e| anyhow::anyhow!("vault parse error: {e}"))?;

    println!("recipient count in header: {}", envelope.recipient_count);
    let listed = runsible_vault::recipients::list_recipients(&raw)
        .map_err(|e| anyhow::anyhow!("listing recipients: {e}"))?;
    for entry in &listed {
        println!("  {entry}");
    }
    Ok(())
}

/// `recipients add <file.vault> --recipient <new> --existing <e1> --existing <e2> ...`
fn cmd_recipients_add(file: &Path, new_recipient: &str, existing: &[String]) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let keys_path = default_keys_path();
    let store = KeyStore::load_or_default(&keys_path).context("loading key store")?;

    if existing.is_empty() {
        anyhow::bail!(
            "recipients add: --existing list is empty. \
             You must list ALL current recipient pubkeys via repeated --existing flags \
             so previously-authorized parties stay authorized after rekey."
        );
    }

    let new_envelope = runsible_vault::recipients::add_recipient(&raw, new_recipient, existing, &store)
        .map_err(|e| anyhow::anyhow!("recipients add: {e}"))?;

    std::fs::write(file, new_envelope.as_bytes())
        .with_context(|| format!("writing {}", file.display()))?;
    println!("added recipient → {}", file.display());
    Ok(())
}

/// `recipients remove <file.vault> --recipient <drop> --existing <e1> ...`
fn cmd_recipients_remove(file: &Path, drop_recipient: &str, existing: &[String]) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let keys_path = default_keys_path();
    let store = KeyStore::load_or_default(&keys_path).context("loading key store")?;

    if existing.is_empty() {
        anyhow::bail!(
            "recipients remove: --existing list is empty. \
             You must list ALL current recipient pubkeys via repeated --existing flags."
        );
    }

    let new_envelope = runsible_vault::recipients::remove_recipient(&raw, drop_recipient, existing, &store)
        .map_err(|e| anyhow::anyhow!("recipients remove: {e}"))?;

    std::fs::write(file, new_envelope.as_bytes())
        .with_context(|| format!("writing {}", file.display()))?;
    println!("removed recipient → {}", file.display());
    Ok(())
}

/// `recipients rekey <file.vault> --recipient <pubkey>...`
fn cmd_recipients_rekey(file: &Path, new_recipients: &[String]) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let keys_path = default_keys_path();
    let store = KeyStore::load_or_default(&keys_path).context("loading key store")?;

    if new_recipients.is_empty() {
        anyhow::bail!(
            "recipients rekey: at least one --recipient is required (refusing to write a vault file with zero recipients)"
        );
    }

    let new_envelope = runsible_vault::recipients::rekey_to(&raw, new_recipients, &store)
        .map_err(|e| anyhow::anyhow!("recipients rekey: {e}"))?;

    std::fs::write(file, new_envelope.as_bytes())
        .with_context(|| format!("writing {}", file.display()))?;
    println!("rekeyed → {}", file.display());
    Ok(())
}

/// `import-ansible <file> [--password-file PATH | --password STR] [-r ...] [-o OUT]`
fn cmd_import_ansible(
    file: &Path,
    password_file: Option<&Path>,
    password: Option<&str>,
    cli_recipients: &[String],
    out: Option<&Path>,
) -> anyhow::Result<()> {
    let body = std::fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;

    // Resolve password: --password-file > --password > stdin.
    let pw = if let Some(p) = password_file {
        std::fs::read_to_string(p)
            .with_context(|| format!("reading password file {}", p.display()))?
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    } else if let Some(p) = password {
        p.to_owned()
    } else {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("reading password from stdin")?;
        buf.trim_end_matches(['\n', '\r']).to_owned()
    };
    if pw.is_empty() {
        anyhow::bail!("ansible-vault import: empty password (use --password-file or --password)");
    }

    let plaintext = runsible_vault::ansible_import::import_ansible_vault(&body, &pw)
        .map_err(|e| anyhow::anyhow!("import-ansible: {e}"))?;

    let recipients = resolve_recipients(cli_recipients)?;
    let recipient_count = recipients.len() as u32;
    let ciphertext = encrypt_bytes_to_keys(&plaintext, &recipients)
        .context("re-encrypting under runsible recipients")?;
    let envelope = emit_envelope(&ciphertext, recipient_count);

    let out_path = out.map(PathBuf::from).unwrap_or_else(|| {
        let mut p = file.as_os_str().to_owned();
        p.push(".vault");
        PathBuf::from(p)
    });
    std::fs::write(&out_path, envelope.as_bytes())
        .with_context(|| format!("writing {}", out_path.display()))?;
    println!("imported → {}", out_path.display());
    Ok(())
}

/// `encrypt-string [--value <v>]`
fn cmd_encrypt_string(value: Option<String>, cli_recipients: &[String]) -> anyhow::Result<()> {
    let plaintext = match value {
        Some(v) => v,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).context("reading stdin")?;
            buf.trim_end_matches('\n').to_owned()
        }
    };

    let recipients = resolve_recipients(cli_recipients)?;
    let snippet = runsible_vault::encrypt_string(&plaintext, &recipients)
        .context("encrypting string")?;

    println!("{snippet}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve recipients: use CLI-provided keys, or fall back to the first key in
/// the default key store.
fn resolve_recipients(cli_recipients: &[String]) -> anyhow::Result<Vec<String>> {
    if !cli_recipients.is_empty() {
        return Ok(cli_recipients.to_vec());
    }

    let keys_path = default_keys_path();
    let store = KeyStore::load_or_default(&keys_path)
        .context("loading key store for default recipient")?;

    let pubkey = store
        .first_public_key()
        .ok_or_else(|| anyhow::anyhow!(
            "no recipients specified and no keys found in {}; run `runsible-vault keygen` first",
            keys_path.display()
        ))?;
    Ok(vec![pubkey])
}
