//! Object-safe module dispatch layer.
//!
//! `DynModule` is the engine's interface to modules; it wraps the typed
//! `runsible_core::Module` trait behind a `Box<dyn DynModule>`.

use indexmap::IndexMap;
use runsible_core::types::{Host, Outcome, Plan};

use crate::errors::Result;

pub trait DynModule: Send + Sync {
    fn module_name(&self) -> &str;
    fn plan(&self, args: &toml::Value, host: &Host) -> Result<Plan>;
    fn apply(&self, plan: &Plan, host: &Host) -> Result<Outcome>;
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
