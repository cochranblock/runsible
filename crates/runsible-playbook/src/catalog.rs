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
