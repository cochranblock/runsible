//! Object-safe module dispatch layer.
//!
//! `DynModule` is the engine's interface to modules; it takes an
//! `ExecutionContext` (host + vars + connection + check_mode) so modules can
//! reach the controller and remote host without dragging in associated types.

use indexmap::IndexMap;
use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, Plan};

use crate::errors::Result;

pub trait DynModule: Send + Sync {
    fn module_name(&self) -> &str;
    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan>;
    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome>;
    /// Whether this module is safe to run in `--check` mode (no host-state
    /// mutation). Default `false` — engine will skip apply() in check_mode for
    /// modules that don't override this.
    ///
    /// Override to `true` for read-only/synthetic modules: `debug`, `ping`,
    /// `set_fact`, `assert`. These mutate engine-side state (vars/notifications)
    /// but not the target host, so they're safe to run.
    fn check_mode_safe(&self) -> bool {
        false
    }
}

pub struct ModuleCatalog {
    modules: IndexMap<String, Box<dyn DynModule>>,
}

impl ModuleCatalog {
    pub fn with_builtins() -> Self {
        let mut c = Self {
            modules: IndexMap::new(),
        };
        c.register(Box::new(crate::modules::debug::DebugModule));
        c.register(Box::new(crate::modules::ping::PingModule));
        c.register(Box::new(crate::modules::set_fact::SetFactModule));
        c.register(Box::new(crate::modules::setup::SetupModule));
        c.register(Box::new(crate::modules::assert_mod::AssertModule));
        c.register(Box::new(crate::modules::command::CommandModule));
        c.register(Box::new(crate::modules::shell::ShellModule));
        c.register(Box::new(crate::modules::copy::CopyModule));
        c.register(Box::new(crate::modules::file::FileModule));
        c.register(Box::new(crate::modules::template::TemplateModule));
        c.register(Box::new(crate::modules::package::PackageModule));
        c.register(Box::new(crate::modules::service::ServiceModule));
        c.register(Box::new(crate::modules::systemd_service::SystemdServiceModule));
        c.register(Box::new(crate::modules::get_url::GetUrlModule));
        c.register(Box::new(crate::modules::lineinfile::LineInFileModule));
        c.register(Box::new(crate::modules::blockinfile::BlockInFileModule));
        c.register(Box::new(crate::modules::replace::ReplaceModule));
        c.register(Box::new(crate::modules::stat::StatModule));
        c.register(Box::new(crate::modules::find::FindModule));
        c.register(Box::new(crate::modules::fail::FailModule));
        c.register(Box::new(crate::modules::pause::PauseModule));
        c.register(Box::new(crate::modules::wait_for::WaitForModule));
        c.register(Box::new(crate::modules::uri::UriModule));
        c.register(Box::new(crate::modules::archive::ArchiveModule));
        c.register(Box::new(crate::modules::unarchive::UnarchiveModule));
        c.register(Box::new(crate::modules::user::UserModule));
        c.register(Box::new(crate::modules::group::GroupModule));
        c.register(Box::new(crate::modules::cron::CronModule));
        c.register(Box::new(crate::modules::hostname::HostnameModule));
        c
    }

    pub fn register(&mut self, m: Box<dyn DynModule>) {
        self.modules.insert(m.module_name().to_string(), m);
    }

    pub fn get(&self, name: &str) -> Option<&dyn DynModule> {
        self.modules.get(name).map(|m| m.as_ref())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }
}
