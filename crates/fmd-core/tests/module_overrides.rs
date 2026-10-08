// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_core::settings::{
    ConnectionSettings, EffectiveLimits, HttpOverrides, LimitOverrides, ModuleLimits,
    ModuleOverrides, ProxyOverride, ProxyOverrideType, effective_limits,
};
use fmd_store::AppDb;
use serde_json::json;

fn global(max_parallel_tasks: u32, threads_per_task: u32) -> ConnectionSettings {
    ConnectionSettings {
        max_parallel_tasks,
        threads_per_task,
        ..ConnectionSettings::default()
    }
}

fn module(tasks: u32, threads: u32, connections: u32) -> ModuleLimits {
    ModuleLimits {
        max_task_limit: tasks,
        max_thread_per_task_limit: threads,
        max_connection_limit: connections,
    }
}

fn overrides(enabled: bool, tasks: u32, threads: u32, connections: u32) -> ModuleOverrides {
    ModuleOverrides {
        enabled,
        limits: LimitOverrides {
            max_task_limit: tasks,
            max_thread_per_task_limit: threads,
            max_connection_limit: connections,
        },
        ..ModuleOverrides::default()
    }
}

// GetMaxTaskLimit / CanCreateTask (baseunits/WebsiteModules.pas:398-420) with the global
// OptionMaxParallel check (baseunits/uDownloadsManager.pas:1798-1803).
#[test]
fn task_limit_precedence() {
    let g = global(4, 1);
    let m = module(1, 0, 0);
    let tasks = |o: Option<&ModuleOverrides>| effective_limits(&m, o, &g).max_tasks;

    assert_eq!(tasks(Some(&overrides(true, 2, 0, 0))), 2);
    assert_eq!(tasks(None), 1);
    // Overrides only apply when enabled, and 0 means "no override".
    assert_eq!(tasks(Some(&overrides(false, 2, 0, 0))), 1);
    assert_eq!(tasks(Some(&overrides(true, 0, 0, 0))), 1);
    // The global limit still caps a module.
    assert_eq!(tasks(Some(&overrides(true, 9, 0, 0))), 4);
    // Module 0 = unlimited → only the global limit applies.
    assert_eq!(effective_limits(&module(0, 0, 0), None, &g).max_tasks, 4);
}

// TTaskThread.GetCurrentLimit (baseunits/uDownloadsManager.pas:868-880), using the
// override-aware GetMaxThreadPerTaskLimit (baseunits/WebsiteModules.pas:406-412).
#[test]
fn thread_limit_precedence() {
    let g = global(1, 3);
    let threads =
        |m: ModuleLimits, o: Option<&ModuleOverrides>| effective_limits(&m, o, &g).threads_per_task;

    assert_eq!(threads(module(0, 0, 0), None), 3);
    assert_eq!(threads(module(0, 2, 0), None), 2);
    assert_eq!(threads(module(0, 8, 0), None), 3);
    assert_eq!(threads(module(0, 2, 0), Some(&overrides(true, 0, 1, 0))), 1);
    assert_eq!(
        threads(module(0, 2, 0), Some(&overrides(false, 0, 1, 0))),
        2
    );
    // Capped by the module's connection limit.
    assert_eq!(threads(module(0, 0, 2), None), 2);
}

// TWebsiteModuleSettings.SetEnabled / SetMaxConnectionLimit
// (baseunits/WebsiteModulesSettings.pas:126-155): when enabled, the override replaces the
// module's connection limit outright, 0 (unlimited) included.
#[test]
fn connection_limit_precedence() {
    let g = global(1, 8);
    let m = module(0, 0, 2);
    let limits = |o: Option<&ModuleOverrides>| effective_limits(&m, o, &g);

    assert_eq!(
        limits(None),
        EffectiveLimits {
            max_tasks: 1,
            threads_per_task: 2,
            max_connections: 2
        }
    );
    assert_eq!(limits(Some(&overrides(false, 0, 0, 5))).max_connections, 2);
    assert_eq!(limits(Some(&overrides(true, 0, 0, 5))).max_connections, 5);
    let unlimited = limits(Some(&overrides(true, 0, 0, 0)));
    assert_eq!(unlimited.max_connections, 0);
    assert_eq!(unlimited.threads_per_task, 8);
}

#[test]
fn overrides_round_trip_through_module_settings() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let repo = db.module_settings();

    assert_eq!(
        ModuleOverrides::load(&repo, "mangadex").unwrap(),
        ModuleOverrides::default()
    );

    // Written by fmd-lua (option values) and the anti-bot hook (cookie jar) beforehand.
    repo.set_option("mangadex", "lang", &json!("en")).unwrap();
    repo.set_cookie_jar("mangadex", Some(b"jar")).unwrap();

    let mut overrides = ModuleOverrides::load(&repo, "mangadex").unwrap();
    assert_eq!(overrides.options.get("lang"), Some(&json!("en")));
    overrides.enabled = true;
    overrides.limits = LimitOverrides {
        max_task_limit: 2,
        max_thread_per_task_limit: 3,
        max_connection_limit: 4,
    };
    overrides.http = HttpOverrides {
        user_agent: "UA".into(),
        cookies: "a=b".into(),
        proxy: ProxyOverride {
            kind: ProxyOverrideType::Socks5,
            host: "proxy.lan".into(),
            port: "1080".into(),
            ..ProxyOverride::default()
        },
    };
    overrides.save(&repo, "mangadex").unwrap();

    assert_eq!(ModuleOverrides::load(&repo, "mangadex").unwrap(), overrides);
    assert_eq!(
        repo.cookie_jar("mangadex").unwrap().as_deref(),
        Some(&b"jar"[..])
    );
    assert_eq!(repo.option("mangadex", "lang").unwrap(), Some(json!("en")));
}
