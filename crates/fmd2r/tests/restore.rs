//! `fmd2r restore`: puts a backup from `GET /api/backup` back while the server is stopped
//! (issue #271, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use assert_cmd::Command;
use axum::body::Body;
use axum::http::Request;
use fmd_core::settings::SettingsService;
use fmd_server::{AppState, build_router};
use fmd_store::{Account, AccountStatus, AppDb, NewFavorite};
use http_body_util::BodyExt;
use predicates::prelude::*;
use serde_json::json;
use tower::ServiceExt;

fn fmd2r() -> Command {
    Command::cargo_bin("fmd2r").unwrap()
}

fn account() -> Account {
    Account {
        module_id: "mangadex".into(),
        enabled: true,
        username: "reader@example.com".into(),
        password: "hunter2-secret".into(),
        cookies: "session=topsecretcookie".into(),
        status: AccountStatus::Valid,
    }
}

/// A set-up install in `dir`: a series in the library, a setting with a secret, a saved login
/// and a stylesheet.
fn original(dir: &Path) {
    let db = AppDb::open(dir.join("app.db")).unwrap();
    db.favorites()
        .create(&NewFavorite {
            module_id: "mangadex".into(),
            title: "Frieren".into(),
            link: "/title/frieren".into(),
            save_to: "/downloads".into(),
            cover_url: None,
        })
        .unwrap();
    SettingsService::load(db.clone())
        .unwrap()
        .update(json!({
            "general": {"language": "nl", "setup_completed": true},
            "comic_server": {"kind": "komga", "url": "http://nas:25600", "api_key": "komga-key"},
        }))
        .unwrap();
    db.accounts(db.cipher()).upsert(&account()).unwrap();
    std::fs::write(dir.join("custom.css"), ":root { --bg: #000; }").unwrap();
}

/// The zip `GET /api/backup` gives for the data folder `dir` (#270).
fn back_up(dir: &Path) -> Vec<u8> {
    let state = AppState::new(AppDb::open(dir.join("app.db")).unwrap())
        .unwrap()
        .with_data_dir(dir);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let req = Request::get("/api/backup").body(Body::empty()).unwrap();
        let res = build_router(state).oneshot(req).await.unwrap();
        assert!(res.status().is_success(), "{}", res.status());
        res.into_body().collect().await.unwrap().to_bytes().to_vec()
    })
}

/// Uploads `zip` to a fresh install in `data` through the setup wizard's `POST /api/setup/restore`.
fn stage_through_setup(data: &Path, zip: Vec<u8>) {
    std::fs::create_dir_all(data).unwrap();
    let state = AppState::new(AppDb::open(data.join("app.db")).unwrap())
        .unwrap()
        .with_data_dir(data);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let req = Request::post("/api/setup/restore")
            .header("content-type", "application/zip")
            .body(Body::from(zip))
            .unwrap();
        let res = build_router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 204);
    });
}

/// Starts `fmd2r serve` on `data`, fetches `path` once it listens, and stops it.
fn serve_and_get(data: &Path, path: &str) -> String {
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("fmd2r"))
        .args([
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--no-module-updates",
            "--data-dir",
        ])
        .arg(data)
        .env("RUST_LOG", "info")
        .env_remove("FMD2R_BIND")
        .env_remove("FMD2R_PASSWORD")
        .env_remove("FMD2R_EXTERNAL_AUTH")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines.next().unwrap().unwrap();
        if let Some(rest) = line.split("listening on ").nth(1) {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    let mut stream = TcpStream::connect(&addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    response
}

/// A backup of [`original`], saved as `<dir>/backup.zip`.
fn backup_file(dir: &Path) -> PathBuf {
    let source = dir.join("source");
    std::fs::create_dir_all(&source).unwrap();
    original(&source);
    let zip = dir.join("backup.zip");
    std::fs::write(&zip, back_up(&source)).unwrap();
    zip
}

#[test]
fn restoring_into_an_empty_data_folder_brings_back_the_library_settings_and_logins() {
    let dir = tempfile::tempdir().unwrap();
    let zip = backup_file(dir.path());
    let data = dir.path().join("data");

    fmd2r()
        .arg("restore")
        .arg(&zip)
        .arg("--data-dir")
        .arg(&data)
        .assert()
        .success();

    let db = AppDb::open(data.join("app.db")).unwrap();
    let titles: Vec<String> = db
        .favorites()
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.title)
        .collect();
    assert_eq!(titles, ["Frieren"]);
    assert_eq!(
        db.accounts(db.cipher()).get("mangadex").unwrap(),
        Some(account())
    );
    let settings = SettingsService::load(db).unwrap().get();
    assert_eq!(settings.general.language, "nl");
    assert!(settings.general.setup_completed);
    assert_eq!(settings.comic_server.api_key.as_deref(), Some("komga-key"));
    assert_eq!(
        std::fs::read_to_string(data.join("custom.css")).unwrap(),
        ":root { --bg: #000; }"
    );
}

/// `zip` with each entry passed through `edit`, which drops the entry by returning `None`.
fn rezip(zip: &Path, edit: impl Fn(&str, Vec<u8>) -> Option<Vec<u8>>) {
    let mut archive = zip::ZipArchive::new(Cursor::new(std::fs::read(zip).unwrap())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if let Some(bytes) = edit(&name, bytes) {
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(&bytes).unwrap();
        }
    }
    std::fs::write(zip, out.finish().unwrap().into_inner()).unwrap();
}

/// `app.db` with its schema version set to `version`.
fn with_schema_version(app_db: Vec<u8>, version: u32) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    std::fs::write(&path, app_db).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
    drop(conn);
    std::fs::read(&path).unwrap()
}

/// An install in `data` other than [`original`]'s, and a snapshot of its folder.
fn existing_install(data: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    std::fs::create_dir_all(data).unwrap();
    let db = AppDb::open(data.join("app.db")).unwrap();
    db.settings()
        .set("general", &json!({"language": "de"}))
        .unwrap();
    drop(db);
    std::fs::write(data.join("custom.css"), "body { color: red; }").unwrap();
    snapshot(data)
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    files.sort();
    files
}

/// Runs `fmd2r restore`, expecting it to refuse with `message`.
fn refused(zip: &Path, data: &Path, message: &str) {
    fmd2r()
        .arg("restore")
        .arg(zip)
        .arg("--data-dir")
        .arg(data)
        .assert()
        .failure()
        .stderr(predicate::str::contains(message))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn a_backup_from_a_newer_fmd2r_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let zip = backup_file(dir.path());
    rezip(&zip, |name, bytes| {
        Some(match name {
            "app.db" => with_schema_version(bytes, 9999),
            _ => bytes,
        })
    });
    let data = dir.path().join("data");
    let before = existing_install(&data);

    refused(
        &zip,
        &data,
        "This backup is from a newer FMD2r; update first.",
    );

    assert_eq!(snapshot(&data), before);
}

#[test]
fn a_backup_without_its_key_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let zip = backup_file(dir.path());
    rezip(&zip, |name, bytes| {
        (name != "accounts.key").then_some(bytes)
    });
    let data = dir.path().join("data");
    let before = existing_install(&data);

    refused(&zip, &data, "it has no accounts.key");

    assert_eq!(snapshot(&data), before);
}

#[test]
fn a_backup_whose_key_does_not_decrypt_its_secrets_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let zip = backup_file(dir.path());
    let data = dir.path().join("data");
    let before = existing_install(&data);
    // Another install's key.
    let foreign = std::fs::read(data.join("accounts.key")).unwrap();
    rezip(&zip, |name, bytes| {
        Some(match name {
            "accounts.key" => foreign.clone(),
            _ => bytes,
        })
    });

    refused(&zip, &data, "accounts.key does not decrypt");

    assert_eq!(snapshot(&data), before);
}

#[test]
fn the_replaced_files_are_kept_as_before_restore() {
    let dir = tempfile::tempdir().unwrap();
    let zip = backup_file(dir.path());
    let data = dir.path().join("data");
    let before = existing_install(&data);

    fmd2r()
        .arg("restore")
        .arg(&zip)
        .arg("--data-dir")
        .arg(&data)
        .assert()
        .success()
        .stdout(predicate::str::contains("before-restore"));

    for (path, bytes) in before {
        let name = path.file_name().unwrap().to_str().unwrap();
        let kept = data.join(format!("{name}.before-restore"));
        assert_eq!(std::fs::read(&kept).unwrap(), bytes, "{name}");
    }
    let old = AppDb::open(data.join("app.db.before-restore")).unwrap();
    assert_eq!(
        old.settings().get::<serde_json::Value>("general").unwrap(),
        Some(json!({"language": "de"}))
    );
    let db = AppDb::open(data.join("app.db")).unwrap();
    assert_eq!(db.favorites().list().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(data.join("custom.css")).unwrap(),
        ":root { --bg: #000; }"
    );
}

#[test]
fn a_backup_staged_by_the_setup_wizard_is_applied_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let zip = std::fs::read(backup_file(dir.path())).unwrap();
    let data = dir.path().join("data");
    stage_through_setup(&data, zip);

    let favorites = serve_and_get(&data, "/api/favorites");

    assert!(favorites.contains("Frieren"), "{favorites}");
    assert!(data.join("app.db.before-restore").is_file());
}
