//! runsible-config — TOML-native configuration for runsible.
//!
//! Search precedence (highest to lowest):
//!   1. `RUNSIBLE_CONFIG` env var (path)
//!   2. `./runsible.toml`
//!   3. `$XDG_CONFIG_HOME/runsible/config.toml` (or `~/.config/runsible/config.toml`)
//!   4. `/etc/runsible/runsible.toml`
//!   5. compiled-in defaults
//!
//! Per §6 of the runsible-config plan: env-var overrides are opt-in per key,
//! never via implicit `RUNSIBLE_*` shadowing.

use std::env;
use std::path::{Path, PathBuf};

use runsible_core::errors::{ConfigError, Result};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// The full runsible config schema. M0: a curated subset of keys.
/// Adding keys is non-breaking; renaming requires a schema_version bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version. Defaults to current when missing.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    #[serde(default)]
    pub defaults: Defaults,

    #[serde(default)]
    pub inventory: Inventory,

    #[serde(default)]
    pub privilege_escalation: PrivilegeEscalation,

    #[serde(default)]
    pub ssh: Ssh,

    #[serde(default)]
    pub vault: Vault,

    #[serde(default)]
    pub galaxy: Galaxy,

    #[serde(default)]
    pub output: Output,

    #[serde(default)]
    pub lint: Lint,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            defaults: Defaults::default(),
            inventory: Inventory::default(),
            privilege_escalation: PrivilegeEscalation::default(),
            ssh: Ssh::default(),
            vault: Vault::default(),
            galaxy: Galaxy::default(),
            output: Output::default(),
            lint: Lint::default(),
        }
    }
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub forks: u32,
    pub timeout_seconds: u32,
    pub poll_interval_seconds: u32,
    pub host_key_checking: HostKeyChecking,
    pub gather_facts: bool,
    pub stdout_callback: String,
    pub display_skipped_hosts: bool,
    pub deprecation_warnings: bool,
    pub system_warnings: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            forks: 20,
            timeout_seconds: 30,
            poll_interval_seconds: 1,
            host_key_checking: HostKeyChecking::AcceptNew,
            gather_facts: false,
            stdout_callback: "auto".into(),
            display_skipped_hosts: false,
            deprecation_warnings: true,
            system_warnings: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyChecking {
    Strict,
    AcceptNew,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub paths: Vec<PathBuf>,
    pub host_pattern_mismatch: PatternMismatch,
    pub any_unparsed_is_failed: bool,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            paths: vec![],
            host_pattern_mismatch: PatternMismatch::Error,
            any_unparsed_is_failed: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatternMismatch {
    Error,
    Warning,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivilegeEscalation {
    pub become_default: bool,
    pub become_user_default: String,
    pub become_method_default: String,
    pub become_ask_pass: bool,
}

impl Default for PrivilegeEscalation {
    fn default() -> Self {
        Self {
            become_default: false,
            become_user_default: "root".into(),
            become_method_default: "sudo".into(),
            become_ask_pass: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ssh {
    pub control_master: ControlMaster,
    pub control_persist_seconds: u32,
    pub control_path: String,
    pub pipelining: bool,
    pub timeout_seconds: u32,
}

impl Default for Ssh {
    fn default() -> Self {
        Self {
            control_master: ControlMaster::Auto,
            control_persist_seconds: 60,
            control_path: "~/.runsible/cm/%r@%h:%p".into(),
            pipelining: true,
            timeout_seconds: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlMaster {
    Auto,
    Yes,
    No,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vault {
    pub keys_path: PathBuf,
    pub recipients_file: Option<PathBuf>,
    pub default_recipients: Vec<String>,
}

impl Default for Vault {
    fn default() -> Self {
        Self {
            keys_path: PathBuf::from("~/.runsible/keys.toml"),
            recipients_file: None,
            default_recipients: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Galaxy {
    pub registry: String,
    pub registry_url: Option<String>,
    pub cache_path: PathBuf,
    pub require_signatures: bool,
}

impl Default for Galaxy {
    fn default() -> Self {
        Self {
            registry: "default".into(),
            registry_url: None,
            cache_path: PathBuf::from("~/.runsible/cache"),
            require_signatures: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub format: OutputFormat,
    pub color: ColorPolicy,
    pub verbosity: u8,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            format: OutputFormat::Auto,
            color: ColorPolicy::Auto,
            verbosity: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Auto,
    Pretty,
    Ndjson,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColorPolicy {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lint {
    pub profile: String,
    pub auto_fix: bool,
}

impl Default for Lint {
    fn default() -> Self {
        Self {
            profile: "basic".into(),
            auto_fix: false,
        }
    }
}

/// Source of a config value, used by `runsible-config explain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    EnvVar(String),
    File(PathBuf),
    Default,
}

/// Result of loading config: the merged Config plus the source of each.
#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub source_path: Option<PathBuf>,
    pub source: Source,
}

/// Search the standard precedence list for a config file. Returns the first
/// match (or None if every search-path step misses).
pub fn search_path() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(p) = env::var("RUNSIBLE_CONFIG") {
        paths.push(PathBuf::from(p));
    }
    paths.push(PathBuf::from("./runsible.toml"));
    if let Some(home) = home_config_dir() {
        paths.push(home.join("runsible/config.toml"));
    }
    paths.push(PathBuf::from("/etc/runsible/runsible.toml"));
    paths
}

fn home_config_dir() -> Option<PathBuf> {
    if let Ok(p) = env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(p));
    }
    env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config"))
}

/// Find the first existing file in the search path.
pub fn find_config_file() -> Option<PathBuf> {
    for p in search_path() {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Load config from disk (or compiled-in defaults if no file found).
pub fn load() -> Result<LoadedConfig> {
    if let Some(path) = find_config_file() {
        load_from_path(&path)
    } else {
        Ok(LoadedConfig {
            config: Config::default(),
            source_path: None,
            source: Source::Default,
        })
    }
}

/// Load from a specific path, with permission and schema validation.
pub fn load_from_path(path: &Path) -> Result<LoadedConfig> {
    check_permissions(path)?;
    let body = std::fs::read_to_string(path)?;
    let config: Config =
        toml::from_str(&body).map_err(|e| ConfigError::InvalidToml {
            path: path.to_path_buf(),
            source: e,
        })?;
    if config.schema_version > SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchemaVersion {
            found: config.schema_version,
            required: SCHEMA_VERSION,
        }
        .into());
    }
    Ok(LoadedConfig {
        config,
        source_path: Some(path.to_path_buf()),
        source: Source::File(path.to_path_buf()),
    })
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    // Ansible's quirk preserved: warn (here: error) on world-writable cwd config files.
    if mode & 0o002 != 0 && path.starts_with(".") {
        return Err(ConfigError::WorldWritable {
            path: path.to_path_buf(),
        }
        .into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Render the full config (with all defaults filled in) as TOML.
pub fn dump_with_defaults(config: &Config) -> std::result::Result<String, toml::ser::Error> {
    toml::to_string_pretty(config)
}

/// Initialize a commented default config file.
pub fn init_default() -> String {
    let header = format!(
        "# runsible.toml — generated by `runsible-config init`\n# schema version: {SCHEMA_VERSION}\n\n"
    );
    let body = toml::to_string_pretty(&Config::default())
        .expect("Config::default() must serialize");
    format!("{header}{body}")
}

/// Smoke gate: exercise the public API end-to-end. Build a default config
/// via `init_default()`, parse it back into a `Config`, verify the schema
/// version and the canonical `defaults.forks == 20`, then round-trip a
/// `Config::default()` through TOML and re-check the same field. Returns 0
/// on success or a non-zero stage code on failure. Used by the
/// runsible-config-test binary's TRIPLE SIMS gate.
pub fn f30() -> i32 {
    // Stage 1: produce the canonical default config text.
    let body = init_default();
    if body.is_empty() {
        return 1;
    }
    // Stage 2: parse it back as Config.
    let parsed: Config = match toml::from_str(&body) {
        Ok(c) => c,
        Err(_) => return 2,
    };
    // Stage 3: schema version must match the compiled-in constant.
    if parsed.schema_version != SCHEMA_VERSION {
        return 3;
    }
    // Stage 4: defaults.forks must be 20 (the canonical Ansible-equivalent).
    if parsed.defaults.forks != 20 {
        return 4;
    }
    // Stage 5: round-trip Config::default() and re-check forks == 20.
    let s = match toml::to_string(&Config::default()) {
        Ok(s) => s,
        Err(_) => return 5,
    };
    let back: Config = match toml::from_str(&s) {
        Ok(c) => c,
        Err(_) => return 6,
    };
    if back.defaults.forks != 20 {
        return 7;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let c = Config::default();
        let s = toml::to_string(&c).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.defaults.forks, 20);
    }

    #[test]
    fn unknown_key_rejected() {
        let s = "[defaults]\nforks = 5\nbogus_key = true\n";
        let r: std::result::Result<Config, _> = toml::from_str(s);
        assert!(r.is_err(), "deny_unknown_fields should reject bogus_key");
    }

    #[test]
    fn schema_version_too_high_errors() {
        let body = format!("schema_version = {}\n", SCHEMA_VERSION + 1);
        let tmp = tempfile_path("runsible-config-test-schema");
        std::fs::write(&tmp, body).unwrap();
        let r = load_from_path(&tmp);
        assert!(r.is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    fn tempfile_path(stem: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "{stem}.{}.toml",
            std::process::id()
        ));
        p
    }

    #[test]
    fn init_default_is_valid_toml() {
        let s = init_default();
        let _: Config = toml::from_str(&s).unwrap();
    }

    // -----------------------------------------------------------------------
    // Config defaults
    // -----------------------------------------------------------------------

    #[test]
    fn defaults_forks_is_20() {
        assert_eq!(Config::default().defaults.forks, 20);
    }

    #[test]
    fn defaults_vault_recipients_file_is_none() {
        assert!(Config::default().vault.recipients_file.is_none());
    }

    #[test]
    fn defaults_output_color_is_auto() {
        assert_eq!(Config::default().output.color, ColorPolicy::Auto);
    }

    // -----------------------------------------------------------------------
    // TOML round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let original = Config::default();
        let serialized = toml::to_string(&original).expect("serialize");
        let parsed: Config = toml::from_str(&serialized).expect("deserialize");

        assert_eq!(parsed.defaults.forks, original.defaults.forks);
        assert_eq!(parsed.defaults.timeout_seconds, original.defaults.timeout_seconds);
        assert_eq!(
            parsed.defaults.poll_interval_seconds,
            original.defaults.poll_interval_seconds
        );
        assert_eq!(parsed.defaults.gather_facts, original.defaults.gather_facts);
        assert_eq!(parsed.schema_version, original.schema_version);
        assert_eq!(parsed.output.color, original.output.color);
        assert_eq!(parsed.ssh.timeout_seconds, original.ssh.timeout_seconds);
    }

    #[test]
    fn partial_toml_falls_back_to_defaults() {
        // Top-level partial: only schema_version provided. Every section
        // (including [defaults], [output], [ssh], ...) is omitted and must
        // therefore fall back to its compiled-in default.
        let s = "schema_version = 1\n";
        let parsed: Config = toml::from_str(s).expect("partial deserialize");
        assert_eq!(parsed.schema_version, 1);
        // Omitted sections fall back to defaults.
        assert_eq!(parsed.defaults.forks, 20);
        assert_eq!(parsed.defaults.timeout_seconds, 30);
        assert_eq!(parsed.defaults.poll_interval_seconds, 1);
        assert_eq!(parsed.output.color, ColorPolicy::Auto);
        assert_eq!(parsed.ssh.timeout_seconds, 10);
    }

    #[test]
    fn unknown_key_in_defaults_section_rejected() {
        let s = "[defaults]\nforks = 5\nmystery_field = 42\n";
        let r: std::result::Result<Config, _> = toml::from_str(s);
        assert!(
            r.is_err(),
            "deny_unknown_fields on Defaults must reject mystery_field"
        );
    }

    // -----------------------------------------------------------------------
    // init_default
    // -----------------------------------------------------------------------

    #[test]
    fn init_default_contains_header_comment() {
        let s = init_default();
        assert!(
            s.contains("# runsible.toml"),
            "init_default output must contain the '# runsible.toml' header"
        );
    }

    #[test]
    fn init_default_parses_with_current_schema_version() {
        let s = init_default();
        let parsed: Config = toml::from_str(&s).expect("init_default must parse");
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.schema_version, 1);
    }

    // -----------------------------------------------------------------------
    // Schema version (load_from_path)
    // -----------------------------------------------------------------------

    fn unique_tempfile(label: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        // Suffix with PID + label + nanos for uniqueness across parallel tests.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!(
            "rsl-cfg-test-{}-{}-{}.toml",
            std::process::id(),
            label,
            nanos
        ));
        p
    }

    #[test]
    fn schema_version_1_accepted_via_load_from_path() {
        let path = unique_tempfile("schema-1");
        std::fs::write(&path, "schema_version = 1\n").expect("write");
        let result = load_from_path(&path);
        let _ = std::fs::remove_file(&path);
        let loaded = result.expect("schema_version = 1 must load");
        assert_eq!(loaded.config.schema_version, 1);
    }

    #[test]
    fn schema_version_99_rejected_via_load_from_path() {
        let path = unique_tempfile("schema-99");
        std::fs::write(&path, "schema_version = 99\n").expect("write");
        let result = load_from_path(&path);
        let _ = std::fs::remove_file(&path);
        assert!(
            result.is_err(),
            "schema_version = 99 must be rejected by load_from_path"
        );
    }

    #[test]
    fn missing_schema_version_defaults_to_current() {
        let path = unique_tempfile("schema-missing");
        // Empty file: no schema_version field at all, no sections.
        // Every top-level section falls back to its default; schema_version
        // falls back to SCHEMA_VERSION via #[serde(default = ...)].
        std::fs::write(&path, "").expect("write");
        let result = load_from_path(&path);
        let _ = std::fs::remove_file(&path);
        let loaded = result.expect("missing schema_version must default");
        assert_eq!(loaded.config.schema_version, SCHEMA_VERSION);
        // And defaults are still in place.
        assert_eq!(loaded.config.defaults.forks, 20);
    }
}
