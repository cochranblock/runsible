//! `Module` and `Connection` traits. These are the contract that the engine
//! uses to drive everything; downstream crates implement these.
//!
//! See docs/plans/runsible-playbook.md §4 and docs/plans/runsible-connection.md §3.

use std::path::Path;
use std::time::Duration;

use serde::{de::DeserializeOwned, Serialize};

use crate::errors::Result;
use crate::types::{Host, Plan};

/// A module: declarative unit of host state.
///
/// `plan()` runs first. If the resulting `Plan::is_empty()`, `apply()` is skipped —
/// that's the type-system enforcement of idempotence (per §9 of poor-decisions).
/// After `apply()`, `verify()` re-derives the plan and asserts it is empty.
pub trait Module {
    type Input: DeserializeOwned + Send + Sync;
    type Outcome: Serialize + Send + Sync;

    /// Module identifier, e.g. `"runsible_builtin.copy"`.
    fn name(&self) -> &'static str;

    /// Whether the module supports check-mode. Defaults to true.
    fn supports_check_mode(&self) -> bool {
        true
    }

    /// Whether the module is expected to be idempotent (i.e., `verify()` is
    /// expected to return Ok(()) on second call). `command`/`shell` say no.
    fn is_idempotent(&self) -> bool {
        true
    }

    fn plan(&self, input: &Self::Input, host: &Host) -> Result<Plan>;
    fn apply(&self, plan: &Plan, host: &Host) -> Result<Self::Outcome>;
    fn verify(&self, plan: &Plan, host: &Host) -> Result<()>;
}

/// Talk to a remote (or local) host. `runsible-connection` provides
/// implementations; the engine consumes the trait.
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    async fn exec(&self, cmd: &Cmd) -> Result<ExecOutcome>;
    async fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> Result<()>;
    async fn get_file(&self, src: &Path, dst: &Path) -> Result<()>;
    async fn slurp(&self, src: &Path) -> Result<Vec<u8>>;
    async fn close(&mut self) -> Result<()>;
}

/// Synchronous connection facade used by the engine + module dispatch.
///
/// The engine is sync; modules need a sync interface they can call without
/// dragging in tokio. `runsible-connection::LocalSync` is the M1 implementation
/// (std::process + std::fs against the controller machine).
pub trait SyncConnection: Send + Sync {
    fn exec(&self, cmd: &Cmd) -> Result<ExecOutcome>;
    fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> Result<()>;
    fn slurp(&self, src: &Path) -> Result<Vec<u8>>;
    fn file_exists(&self, path: &Path) -> Result<bool>;
}

/// Bundle of everything a module needs at plan/apply time.
///
/// Constructed by the engine for each (host, task) pair.
pub struct ExecutionContext<'a> {
    pub host: &'a crate::types::Host,
    pub vars: &'a crate::types::Vars,
    pub connection: &'a dyn SyncConnection,
    pub check_mode: bool,
}

#[derive(Debug, Clone)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<std::path::PathBuf>,
    pub become_: Option<BecomeSpec>,
    pub timeout: Option<Duration>,
    pub tty: bool,
}

#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub rc: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub signal: Option<i32>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct BecomeSpec {
    pub method: BecomeMethod,
    pub user: String,
    pub flags: Vec<String>,
    pub password: Option<SecretSource>,
    pub preserve_env: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BecomeMethod {
    Sudo,
    Su,
    Doas,
    Pbrun,
    Pfexec,
    Dzdo,
    Ksu,
    Runas,
    Machinectl,
    Sesu,
}

#[derive(Debug, Clone)]
pub enum SecretSource {
    Keyring { service: String, key: String },
    Env(String),
    Plaintext(String),
}
