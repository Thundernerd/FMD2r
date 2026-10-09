//! The loaded Lua modules as the endpoints see them: the module list, series info run on the
//! shared worker pool, and HTTP sessions for their covers.

use std::sync::{Arc, Mutex, PoisonError};

use fmd_core::info::{self, InfoError, InfoOptions, MangaInfo};
use fmd_core::module_updater::LiveModules;
use fmd_core::modules::ModuleInfo;
use fmd_core::settings::StoredModuleHttpSettings;
use fmd_http::HttpClient;
use fmd_lua::{HttpModule, ModuleRegistry, WorkerPool, XPathBackend, create_http};
use fmd_store::AppDb;
use futures_util::future::BoxFuture;

use crate::covers::{CoverModules, CoverSession};
use crate::module_updates::LuaRuntime;
use crate::services::{ModuleCatalog, ModulesReport};

/// The modules of a [`LuaRuntime`], following its reloads.
#[derive(Clone)]
pub(crate) struct LuaCatalog {
    modules: Arc<LiveModules>,
    pool: Arc<WorkerPool>,
    http: HttpClient,
    /// Where the modules' HTTP settings are stored.
    db: AppDb,
    /// The module list of the registry it was built from, rebuilt when a reload swaps it.
    infos: Arc<Mutex<Option<ModuleList>>>,
}

/// The module list of one registry.
struct ModuleList {
    built_from: Arc<ModuleRegistry>,
    infos: Arc<Vec<ModuleInfo>>,
}

impl LuaCatalog {
    /// The modules of `runtime`, their HTTP settings stored in `db`.
    pub(crate) fn new(runtime: &LuaRuntime, db: AppDb) -> LuaCatalog {
        LuaCatalog {
            modules: runtime.modules.clone(),
            pool: runtime.pool.clone(),
            http: runtime.http.clone(),
            db,
            infos: Arc::default(),
        }
    }

    /// Every loaded module, sorted by ID; built once per registry.
    fn infos(&self) -> Arc<Vec<ModuleInfo>> {
        let registry = self.modules.current();
        let mut cached = self.infos.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(list) = cached.as_ref()
            && Arc::ptr_eq(&list.built_from, &registry)
        {
            return list.infos.clone();
        }
        // The registry keeps its modules sorted by ID.
        let infos: Arc<Vec<ModuleInfo>> = Arc::new(
            registry
                .modules()
                .iter()
                .map(|m| ModuleInfo::from(&m.def()))
                .collect(),
        );
        *cached = Some(ModuleList {
            built_from: registry,
            infos: infos.clone(),
        });
        infos
    }
}

impl ModuleCatalog for LuaCatalog {
    fn report(&self) -> ModulesReport {
        ModulesReport {
            module_count: self.infos().len() as u64,
            xpath_backend: Some(
                match self.pool.xpath_backend().unwrap_or_default() {
                    XPathBackend::Fpc => "fpc",
                    XPathBackend::Native => "native",
                }
                .to_owned(),
            ),
            ..ModulesReport::default()
        }
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        self.infos().as_ref().clone()
    }

    fn get_info(
        &self,
        id: &str,
        link: &str,
        options: InfoOptions,
    ) -> BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let module = self.modules.current().get(id).cloned();
        let pool = self.pool.clone();
        let link = link.to_owned();
        Box::pin(async move {
            let module = module.ok_or(InfoError::UnknownModule)?;
            info::get_info(&pool, &module, &link, options).await
        })
    }
}

impl CoverModules for LuaCatalog {
    /// A session like the one `TModuleContainer.CreateHTTP` gives a module
    /// (baseunits/WebsiteModules.pas:353-387): its cookie jar, connection queue, and the user
    /// agent, proxy and cookies of its settings.
    fn cover_session(&self, id: &str) -> Option<CoverSession> {
        let module = self.modules.current().get(id).cloned()?;
        let module_http = HttpModule {
            http: module.http().clone(),
            settings: Arc::new(StoredModuleHttpSettings::new(self.db.clone(), id)),
        };
        Some(CoverSession {
            root_url: module.def().root_url,
            session: create_http(&self.http, Some(&module_http)),
        })
    }
}
