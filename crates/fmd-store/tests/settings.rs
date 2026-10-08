use fmd_store::{AppDb, ModuleFile, ModuleSettings};
use serde_json::json;

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Connections {
    max_tasks: u32,
    threads_per_task: u32,
}

#[test]
fn settings_round_trip_typed_values() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let settings = db.settings();

    assert_eq!(settings.get::<Connections>("connections").unwrap(), None);
    let value = Connections {
        max_tasks: 4,
        threads_per_task: 2,
    };
    settings.set("connections", &value).unwrap();
    settings.set("language", &"en").unwrap();
    assert_eq!(settings.get("connections").unwrap(), Some(value));
    assert_eq!(settings.get::<String>("language").unwrap().as_deref(), Some("en"));

    settings.set("language", &"nl").unwrap();
    assert_eq!(settings.get::<String>("language").unwrap().as_deref(), Some("nl"));
    assert!(settings.get::<u32>("language").is_err());

    settings.remove("language").unwrap();
    assert_eq!(settings.get::<String>("language").unwrap(), None);
}

#[test]
fn module_settings_store_options_overrides_and_cookie_jar() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let repo = db.module_settings();

    assert_eq!(repo.get("mangadex").unwrap(), None);
    assert_eq!(repo.option("mangadex", "hq").unwrap(), None);

    repo.set_option("mangadex", "hq", &json!(true)).unwrap();
    repo.set_option("mangadex", "lang", &json!("en")).unwrap();
    assert_eq!(repo.option("mangadex", "hq").unwrap(), Some(json!(true)));
    repo.set_cookie_jar("mangadex", Some(b"jar-bytes")).unwrap();

    let stored = repo.get("mangadex").unwrap().unwrap();
    assert_eq!(stored.options, json!({ "hq": true, "lang": "en" }));
    assert_eq!(stored.cookie_jar.as_deref(), Some(&b"jar-bytes"[..]));
    assert!(!stored.enabled);

    let updated = ModuleSettings {
        enabled: true,
        http: json!({ "user_agent": "UA", "proxy": { "type": "socks5" } }),
        limits: json!({ "max_connections": 2 }),
        ..stored
    };
    repo.upsert(&updated).unwrap();
    assert_eq!(repo.get("mangadex").unwrap(), Some(updated));
    assert_eq!(repo.option("mangadex", "lang").unwrap(), Some(json!("en")));
}

#[test]
fn module_files_upsert_list_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let files = db.module_files();
    let file = |path: &str, sha: &str| ModuleFile {
        path: path.into(),
        sha: sha.into(),
        last_modified: 1_700_000_000_000,
        size: 1234,
    };

    files.upsert(&file("modules/MangaDex.lua", "aaa")).unwrap();
    files.upsert(&file("lua/utils/json.lua", "bbb")).unwrap();
    files.upsert(&file("modules/MangaDex.lua", "ccc")).unwrap();

    assert_eq!(
        files.get("modules/MangaDex.lua").unwrap(),
        Some(file("modules/MangaDex.lua", "ccc"))
    );
    let all = files.list().unwrap();
    assert_eq!(
        all,
        [file("lua/utils/json.lua", "bbb"), file("modules/MangaDex.lua", "ccc")]
    );

    files.delete("lua/utils/json.lua").unwrap();
    assert_eq!(files.list().unwrap().len(), 1);
}
