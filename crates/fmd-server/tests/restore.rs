//! `POST /api/setup/restore`: the setup wizard's upload is validated and staged, and the next
//! start applies it (issue #271, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Read, Write};
use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use fmd_core::settings::SettingsService;
use fmd_server::{AppState, ImportLimits, apply_staged_restore, build_router};
use fmd_store::{Account, AccountStatus, AppDb, NewFavorite};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

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

/// A state with its data folder in a temp dir.
fn fresh_install() -> (TempDir, AppDb, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let state = AppState::new(db.clone()).unwrap().with_data_dir(dir.path());
    (dir, db, state)
}

/// A backup, from `GET /api/backup`, of an install with a series, settings and a saved login.
async fn backup() -> Vec<u8> {
    let (dir, db, state) = fresh_install();
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
        .update(json!({"general": {"language": "nl", "setup_completed": true}}))
        .unwrap();
    db.accounts(db.cipher()).upsert(&account()).unwrap();
    let req = Request::get("/api/backup").body(Body::empty()).unwrap();
    let res = build_router(state).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    drop(dir);
    bytes
}

async fn upload(state: &AppState, zip: Vec<u8>) -> Response {
    let req = Request::post("/api/setup/restore")
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from(zip))
        .unwrap();
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn detail(res: Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let problem: Value = serde_json::from_slice(&bytes).unwrap();
    problem["detail"].as_str().unwrap().to_owned()
}

/// `zip` without the entry `drop`.
fn without(zip: &[u8], drop: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name() == drop {
            continue;
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn titles(data: &Path) -> Vec<String> {
    let db = AppDb::open(data.join("app.db")).unwrap();
    db.favorites()
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.title)
        .collect()
}

#[tokio::test]
async fn the_upload_is_staged_and_the_next_start_applies_it() {
    let (dir, db, state) = fresh_install();
    let zip = backup().await;

    let res = upload(&state, zip).await;

    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // The running server keeps its data until the restart.
    assert!(db.favorites().list().unwrap().is_empty());
    drop((db, state));

    assert!(apply_staged_restore(dir.path()).unwrap());

    assert_eq!(titles(dir.path()), ["Frieren"]);
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    assert_eq!(
        db.accounts(db.cipher()).get("mangadex").unwrap(),
        Some(account())
    );
    let settings = SettingsService::load(db).unwrap().get();
    assert_eq!(settings.general.language, "nl");
    // Applied once.
    assert!(!apply_staged_restore(dir.path()).unwrap());
}

#[tokio::test]
async fn an_invalid_backup_is_refused_with_its_message_and_nothing_is_staged() {
    let (dir, _db, state) = fresh_install();
    let zip = without(&backup().await, "accounts.key");

    let res = upload(&state, zip).await;

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        detail(res).await,
        "not a backup from FMD2r: it has no accounts.key"
    );
    assert!(!apply_staged_restore(dir.path()).unwrap());
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with("app.db"))
        .collect();
    left.sort();
    assert_eq!(left, ["accounts.key"], "upload leftovers");
}

#[tokio::test]
async fn a_server_that_is_set_up_refuses_the_upload() {
    let (dir, _db, state) = fresh_install();
    state
        .settings()
        .update(json!({"general": {"setup_completed": true}}))
        .unwrap();

    let res = upload(&state, backup().await).await;

    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(detail(res).await.contains("fmd2r restore"));
    assert!(!apply_staged_restore(dir.path()).unwrap());
}

#[tokio::test]
async fn uploads_are_size_limited_like_the_import() {
    let (dir, _db, state) = fresh_install();
    let zip = backup().await;
    let small = state.clone().with_import_limits(ImportLimits {
        upload_bytes: 100,
        extracted_bytes: 1 << 30,
    });

    let res = upload(&small, zip.clone()).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let unpacked = state.with_import_limits(ImportLimits {
        upload_bytes: 1 << 30,
        extracted_bytes: 1000,
    });
    let res = upload(&unpacked, zip).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(detail(res).await.contains("unpacked"));
    assert!(!apply_staged_restore(dir.path()).unwrap());
}
