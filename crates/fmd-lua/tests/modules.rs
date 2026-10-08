//! Loading website modules with `ModuleRegistry::load_dir` and the `MODULE` object they see
//! (docs/tickets/T06-module-loader-module-object.md, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::fs;

use fmd_lua::{LoadReport, ModuleRegistry, OptionKind};

/// Loads a lua dir whose `modules/` holds `modules` (file name, source).
fn load(modules: &[(&str, &str)]) -> (LoadReport, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("modules")).unwrap();
    for (name, source) in modules {
        fs::write(dir.path().join("modules").join(name), source).unwrap();
    }
    (ModuleRegistry::load_dir(dir.path()), dir)
}

const SITE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'abc'; m.Name = 'Site'; m.RootURL = 'https://EXAMPLE.com'; m.Category = 'English'
  m.OnGetInfo = 'GetInfo'; m.MaxTaskLimit = 2
  m.AddOptionCheckBox('hq', 'High quality', true)
  local bad = NewWebsiteModule()   -- no ID/Name: dropped
end
"#;

#[test]
fn init_values_become_a_module_def() {
    let (report, dir) = load(&[("Site.lua", SITE)]);

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let modules = report.registry.modules();
    assert_eq!(modules.len(), 1);
    let def = modules[0].def();
    assert_eq!(def.id, "abc");
    assert_eq!(def.name, "Site");
    assert_eq!(def.root_url, "https://example.com");
    assert_eq!(def.category, "English");
    assert_eq!(def.on_get_info.as_deref(), Some("GetInfo"));
    assert_eq!(def.on_get_page_number, None);
    assert_eq!(def.max_task_limit, 2);
    assert_eq!(def.file, dir.path().join("modules/Site.lua"));
    assert_eq!(def.options.len(), 1);
    assert_eq!(def.options[0].name, "hq");
    assert_eq!(def.options[0].caption, "High quality");
    assert_eq!(def.options[0].kind, OptionKind::CheckBox { default: true });
}

/// A module file declaring one module with `id` and `name`.
fn module(id: &str, name: &str) -> String {
    format!("function Init() local m = NewWebsiteModule(); m.ID = '{id}'; m.Name = '{name}' end")
}

#[test]
fn every_failing_file_is_reported_and_the_others_still_load() {
    let good = module("good", "Good");
    let (report, dir) = load(&[
        ("Good.lua", &good),
        ("Syntax.lua", "function Init( end"),
        ("Raises.lua", "error('top level')"),
        ("NoInit.lua", "local x = 1"),
        ("InitRaises.lua", "function Init() error('in init') end"),
        ("notes.txt", "not a module"),
    ]);

    assert_eq!(report.files, 5);
    let ids: Vec<_> = report
        .registry
        .modules()
        .iter()
        .map(|m| m.def().id)
        .collect();
    assert_eq!(ids, ["good"]);
    let failures: Vec<_> = report
        .failures
        .iter()
        .map(|f| {
            (
                f.file
                    .strip_prefix(dir.path().join("modules"))
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
                f.error.clone(),
            )
        })
        .collect();
    assert_eq!(failures.len(), 4, "{failures:?}");
    let error = |file: &str| &failures.iter().find(|(f, _)| f == file).unwrap().1;
    assert!(
        error("Syntax.lua").starts_with("luaL_loadfile: "),
        "{failures:?}"
    );
    assert!(
        error("Raises.lua").starts_with("lua_pcall: "),
        "{failures:?}"
    );
    assert!(error("Raises.lua").contains("top level"), "{failures:?}");
    assert_eq!(error("NoInit.lua"), r#"no function name "Init()""#);
    assert!(error("InitRaises.lua").contains("in init"), "{failures:?}");
}

#[test]
fn one_file_may_declare_several_modules() {
    let (report, _dir) = load(&[(
        "Multi.lua",
        r#"
        function Init()
          for i, name in ipairs({ 'Beta', 'Alpha', '' }) do
            local m = NewWebsiteModule()
            m.ID = 'id-' .. name; m.Name = name; m.RootURL = 'HTTPS://' .. name .. '.Example.COM'
          end
          local noid = NewWebsiteModule(); noid.Name = 'No ID'
        end
        "#,
    )]);

    let defs: Vec<_> = report.registry.modules().iter().map(|m| m.def()).collect();
    let summary: Vec<_> = defs
        .iter()
        .map(|d| (d.id.as_str(), d.root_url.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("id-Alpha", "https://alpha.example.com"),
            ("id-Beta", "https://beta.example.com"),
        ]
    );
}

#[test]
fn modules_created_before_init_fails_are_kept() {
    let (report, _dir) = load(&[(
        "Partial.lua",
        "function Init() local m = NewWebsiteModule(); m.ID = 'p'; m.Name = 'P'; error('late') end",
    )]);

    assert_eq!(report.registry.modules().len(), 1);
    assert_eq!(report.failures.len(), 1);
}

#[test]
fn modules_can_require_files_from_the_lua_dir_during_init() {
    let (report, dir) = load(&[(
        "UsesTemplate.lua",
        "function Init() local m = NewWebsiteModule(); m.ID = 't'; m.Name = require 'templates.base'.name end",
    )]);
    assert_eq!(
        report.failures.len(),
        1,
        "templates/base.lua does not exist yet"
    );

    fs::create_dir_all(dir.path().join("templates")).unwrap();
    fs::write(
        dir.path().join("templates/base.lua"),
        "return { name = 'From template' }",
    )
    .unwrap();
    let report = ModuleRegistry::load_dir(dir.path());

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        report.registry.get("t").unwrap().def().name,
        "From template"
    );
}

/// A runtime whose `MODULE` is `module`, as a callback would see it.
fn runtime_for(module: &std::sync::Arc<fmd_lua::Module>) -> fmd_lua::Runtime {
    let runtime = fmd_lua::Runtime::new().unwrap();
    runtime.set_module(module).unwrap();
    runtime
}

#[test]
fn storage_is_shared_by_every_state_running_a_module() {
    let (report, _dir) = load(&[("Site.lua", SITE)]);
    let module = report.registry.get("abc").unwrap();

    runtime_for(module)
        .exec("MODULE.Storage['k'] = 'v'")
        .unwrap();

    let second = runtime_for(module);
    assert_eq!(second.eval::<String>("MODULE.Storage['k']").unwrap(), "v");
    assert_eq!(module.storage_value("k"), b"v");
    second
        .exec(
            r#"
            MODULE.Storage['n'] = 1
            assert(MODULE.Storage.Text == 'k=v\r\nn=1\r\n', MODULE.Storage.Text)
            MODULE.Storage.Remove('k')
            assert(MODULE.Storage['k'] == '')
            MODULE.Storage.Text = 'a=b'
            assert(MODULE.Storage['a'] == 'b')
            "#,
        )
        .unwrap();
}

#[test]
fn storage_and_guardian_are_shared_across_threads_and_isolated_between_modules() {
    let other = module("other", "Other");
    let (report, _dir) = load(&[("Site.lua", SITE), ("Other.lua", &other)]);
    let site = report.registry.get("abc").unwrap().clone();
    let other = report.registry.get("other").unwrap().clone();

    runtime_for(&site)
        .exec("MODULE.Storage['token'] = 'abc-token'; MODULE.Guardian.Enter()")
        .unwrap();
    let seen = std::thread::scope(|s| {
        s.spawn(|| {
            let site_rt = runtime_for(&site);
            let other_rt = runtime_for(&other);
            (
                site_rt.eval::<String>("MODULE.Storage['token']").unwrap(),
                site_rt.eval::<bool>("MODULE.Guardian.TryEnter()").unwrap(),
                other_rt.eval::<String>("MODULE.Storage['token']").unwrap(),
                other_rt.eval::<bool>("MODULE.Guardian.TryEnter()").unwrap(),
            )
        })
        .join()
        .unwrap()
    });

    assert_eq!(seen, ("abc-token".to_owned(), false, String::new(), true));
}

#[test]
fn guardian_enter_and_leave_hand_the_lock_between_threads() {
    let (report, _dir) = load(&[("Site.lua", SITE)]);
    let module = report.registry.get("abc").unwrap().clone();
    let runtime = runtime_for(&module);
    runtime
        .exec("MODULE.Guardian.Enter(); MODULE.Guardian.Enter(); MODULE.Guardian.Leave()")
        .unwrap();
    let try_enter = || {
        std::thread::scope(|s| {
            s.spawn(|| {
                runtime_for(&module)
                    .eval::<bool>(
                        "MODULE.Guardian.TryEnter() and (MODULE.Guardian.Leave() or true)",
                    )
                    .unwrap()
            })
            .join()
            .unwrap()
        })
    };
    assert!(!try_enter(), "entered twice, left once: still held");

    runtime.exec("MODULE.Guardian.Leave()").unwrap();

    assert!(try_enter());
}

#[test]
fn get_option_returns_the_default_until_a_value_is_stored() {
    let store = std::sync::Arc::new(fmd_lua::MemorySettingsStore::new());
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("modules")).unwrap();
    fs::write(
        dir.path().join("modules/Options.lua"),
        r#"
        function Init()
          local m = NewWebsiteModule(); m.ID = 'opt'; m.Name = 'Options'
          m.AddOptionCheckBox('hq', 'High quality', true)
          m.AddOptionEdit('lang', 'Language', 'en')
          m.AddOptionSpinEdit('delay', 'Delay', '3')
          m.AddOptionComboBox('server', 'Server', 'One\nTwo', 1)
          assert(m.GetOption('hq') == true)
        end
        "#,
    )
    .unwrap();
    let report = ModuleRegistry::load_dir_with(dir.path(), store.clone());
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let module = report.registry.get("opt").unwrap();
    let runtime = runtime_for(module);

    runtime
        .exec(
            r#"
            assert(MODULE.GetOption('hq') == true)
            assert(MODULE.GetOption('HQ') == true, 'option names ignore case')
            assert(MODULE.GetOption('lang') == 'en')
            assert(math.type(MODULE.GetOption('delay')) == 'integer' and MODULE.GetOption('delay') == 3)
            assert(MODULE.GetOption('server') == 1)
            assert(MODULE.GetOption('missing') == nil)
            "#,
        )
        .unwrap();

    use fmd_lua::{ModuleSettingsStore, OptionValue};
    store
        .set_option("opt", "hq", OptionValue::Bool(false))
        .unwrap();
    store
        .set_option("opt", "lang", OptionValue::Text("de".into()))
        .unwrap();
    store
        .set_option("opt", "delay", OptionValue::Integer(9))
        .unwrap();
    store
        .set_option("opt", "server", OptionValue::Bool(true))
        .unwrap();
    runtime
        .exec(
            r#"
            assert(MODULE.GetOption('hq') == false)
            assert(MODULE.GetOption('lang') == 'de')
            assert(MODULE.GetOption('delay') == 9)
            assert(MODULE.GetOption('server') == 1, 'a value of the wrong type is ignored')
            "#,
        )
        .unwrap();
    let def = module.def();
    let kinds: Vec<_> = def
        .options
        .iter()
        .map(|o| (o.name.as_str(), o.kind.clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("hq", OptionKind::CheckBox { default: true }),
            (
                "lang",
                OptionKind::Edit {
                    default: "en".into()
                }
            ),
            ("delay", OptionKind::SpinEdit { default: 3 }),
            (
                "server",
                OptionKind::ComboBox {
                    items: "One\nTwo".into(),
                    default: 1
                }
            ),
        ]
    );
}

#[test]
fn module_has_every_property_and_method_of_fmd2() {
    let (report, _dir) = load(&[("Site.lua", SITE)]);
    let runtime = runtime_for(report.registry.get("abc").unwrap());

    // luaWebsiteModuleAddMetaTable (baseunits/lua/LuaWebsiteModules.pas:991-1040).
    runtime
        .exec(
            r#"
            local strings = { 'ID', 'Name', 'RootURL', 'Category', 'LastUpdated',
              'OnBeforeUpdateList', 'OnAfterUpdateList', 'OnGetDirectoryPageNumber',
              'OnGetNameAndLink', 'OnGetInfo', 'OnTaskStart', 'OnGetPageNumber', 'OnGetImageURL',
              'OnBeforeDownloadImage', 'OnDownloadImage', 'OnSaveImage', 'OnAfterImageSaved',
              'OnLogin', 'OnAccountState', 'OnCheckSite' }
            for _, p in ipairs(strings) do
              assert(type(MODULE[p]) == 'string', p)
              MODULE[p] = 'x' .. p
              assert(MODULE[p] == 'x' .. p, p)
            end
            local integers = { 'MaxTaskLimit', 'MaxThreadPerTaskLimit', 'MaxConnectionLimit',
              'ActiveTaskCount', 'CurrentDirectoryIndex', 'TotalDirectory', 'Tag' }
            for _, p in ipairs(integers) do
              assert(math.type(MODULE[p]) == 'integer', p)
              MODULE[p] = 7
              assert(MODULE[p] == 7, p)
            end
            assert(MODULE.ActiveConnectionCount == 0)
            local booleans = { 'SortedList', 'InformationAvailable', 'FavoriteAvailable',
              'DynamicPageLink', 'AccountSupport' }
            for _, p in ipairs(booleans) do
              assert(type(MODULE[p]) == 'boolean', p)
            end
            local methods = { 'AddOptionCheckBox', 'AddOptionEdit', 'AddOptionSpinEdit',
              'AddOptionComboBox', 'AddServerCookies', 'GetServerCookies', 'RemoveCookies',
              'ClearCookies', 'GetOption' }
            for _, m in ipairs(methods) do assert(type(MODULE[m]) == 'function', m) end
            assert(MODULE.Storage and MODULE.Guardian)
            assert(MODULE.Account == nil, 'no account without AccountSupport')
            "#,
        )
        .unwrap();
}

#[test]
fn defaults_are_those_of_a_new_module_container() {
    let (report, _dir) = load(&[("Plain.lua", &module("plain", "Plain"))]);

    let def = report.registry.get("plain").unwrap().def();

    // TModuleContainer.Create (baseunits/WebsiteModules.pas:325-343).
    assert!(def.information_available && def.favorite_available);
    assert!(!def.sorted_list && !def.dynamic_page_link && !def.account_support);
    assert_eq!(def.total_directory, 1);
    assert_eq!((def.max_task_limit, def.current_directory_index), (0, 0));
}

#[test]
fn account_is_present_once_account_support_is_on() {
    let (report, _dir) = load(&[(
        "Login.lua",
        r#"
        function Init()
          local m = NewWebsiteModule(); m.ID = 'login'; m.Name = 'Login'
          m.AccountSupport = true
          m.OnLogin = 'Login'
          assert(m.Account == nil, 'the Init object was built before AccountSupport')
        end
        "#,
    )]);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let module = report.registry.get("login").unwrap();
    assert!(module.def().account_support);

    runtime_for(module)
        .exec(
            r#"
            local a = MODULE.Account
            assert(a.Enabled == false and a.Username == '' and a.Status == 0)
            a.Enabled = true; a.Username = 'me'; a.Password = 'secret'; a.Status = 2
            a.Cookies = 'sid=1'
            a.Guardian.Enter(); a.Guardian.Leave()
            "#,
        )
        .unwrap();

    let account = module.account().unwrap().state();
    assert!(account.enabled);
    assert_eq!(
        (
            account.username.as_str(),
            account.password.as_str(),
            account.status,
            account.cookies.as_str()
        ),
        ("me", "secret", 2, "sid=1")
    );
    runtime_for(module)
        .exec("assert(MODULE.Account.Username == 'me')")
        .unwrap();
}

#[test]
fn cookies_are_kept_in_the_settings_store() {
    use fmd_lua::ModuleSettingsStore;
    let store = std::sync::Arc::new(fmd_lua::MemorySettingsStore::new());
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("modules")).unwrap();
    fs::write(dir.path().join("modules/Site.lua"), SITE).unwrap();
    let report = ModuleRegistry::load_dir_with(dir.path(), store.clone());
    let module = report.registry.get("abc").unwrap();

    runtime_for(module)
        .exec(
            r#"
            MODULE.AddServerCookies('https://example.com/', 'a=1; path=/')
            MODULE.AddServerCookies('https://example.com/', 'b=2; path=/')
            assert(MODULE.GetServerCookies('example.com', 'a') == 'a=1; domain=example.com; path=/')
            MODULE.RemoveCookies('example.com', 'b')
            "#,
        )
        .unwrap();
    let saved = store.cookies("abc").unwrap().unwrap();

    let reloaded = ModuleRegistry::load_dir_with(dir.path(), store.clone());
    runtime_for(reloaded.registry.get("abc").unwrap())
        .exec(
            r#"
            assert(MODULE.GetServerCookies('example.com') == 'a=1; domain=example.com; path=/')
            MODULE.ClearCookies()
            assert(MODULE.GetServerCookies('example.com') == '')
            "#,
        )
        .unwrap();
    assert!(saved.contains("\"a\""), "{saved}");
    assert!(!store.cookies("abc").unwrap().unwrap().contains("\"a\""));
}

#[test]
fn module_files_load_like_lual_loadfile_skipping_a_bom_and_a_hash_line() {
    let bom = format!("\u{feff}{}", module("bom", "Bom"));
    let hash = format!("#!/usr/bin/env lua\n{}", module("hash", "Hash"));
    let (report, _dir) = load(&[("Bom.lua", &bom), ("Hash.lua", &hash)]);

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.registry.modules().len(), 2);
}
