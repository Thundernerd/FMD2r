//! `GET /api/backup`: a zip of `app.db`, its key and the stylesheet (issue #270, "Seams under
//! test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Read};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::{Account, AccountStatus, AppDb};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

struct Harness {
    dir: TempDir,
    db: AppDb,
    state: AppState,
}

/// A state with its data folder in a temp dir.
fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let state = AppState::new(db.clone()).unwrap().with_data_dir(dir.path());
    Harness { dir, db, state }
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

async fn get_backup(state: &AppState) -> Response {
    let req = Request::get("/api/backup").body(Body::empty()).unwrap();
    build_router(state.clone()).oneshot(req).await.unwrap()
}

type Archive = zip::ZipArchive<Cursor<Vec<u8>>>;

async fn archive(res: Response) -> Archive {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap()
}

fn names(archive: &Archive) -> Vec<String> {
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    names
}

fn entry(archive: &mut Archive, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

fn header_of(res: &Response, name: header::HeaderName) -> &str {
    res.headers().get(name).unwrap().to_str().unwrap()
}

#[tokio::test]
async fn the_zip_holds_app_db_its_key_and_backup_json_named_after_the_day() {
    let h = harness();
    for other in ["lists.db", "metadata.db"] {
        std::fs::write(h.dir.path().join(other), b"rebuildable").unwrap();
    }

    let res = get_backup(&h.state).await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(header_of(&res, header::CONTENT_TYPE), "application/zip");
    let disposition = header_of(&res, header::CONTENT_DISPOSITION).to_owned();
    let name = disposition
        .strip_prefix("attachment; filename=\"fmd2r-backup-")
        .and_then(|rest| rest.strip_suffix(".zip\""))
        .unwrap_or_else(|| panic!("unexpected Content-Disposition {disposition:?}"));
    assert!(
        name.len() == 10
            && name.char_indices().all(|(i, c)| match i {
                4 | 7 => c == '-',
                _ => c.is_ascii_digit(),
            }),
        "{name:?} is not YYYY-MM-DD"
    );
    let mut zip = archive(res).await;
    assert_eq!(names(&zip), ["accounts.key", "app.db", "backup.json"]);
    assert_eq!(
        entry(&mut zip, "accounts.key"),
        std::fs::read(h.dir.path().join("accounts.key")).unwrap()
    );
    let info: Value = serde_json::from_slice(&entry(&mut zip, "backup.json")).unwrap();
    assert_eq!(info["fmd2r_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["schema_version"], h.db.schema_version().unwrap());
    let created = info["created_at"].as_str().unwrap();
    assert!(
        created.starts_with(name),
        "created_at {created:?} is not on {name}"
    );
}

#[tokio::test]
async fn custom_css_is_included_when_present() {
    let h = harness();
    std::fs::write(h.dir.path().join("custom.css"), ":root { --bg: #000; }").unwrap();

    let mut zip = archive(get_backup(&h.state).await).await;

    assert_eq!(
        names(&zip),
        ["accounts.key", "app.db", "backup.json", "custom.css"]
    );
    assert_eq!(entry(&mut zip, "custom.css"), b":root { --bg: #000; }");
}

#[tokio::test]
async fn the_copy_s_secrets_decrypt_with_the_zipped_key() {
    let h = harness();
    h.db.accounts(h.db.cipher()).upsert(&account()).unwrap();
    h.db.settings().set("general.language", "nl").unwrap();

    let mut zip = archive(get_backup(&h.state).await).await;

    let restored = tempfile::tempdir().unwrap();
    for name in ["app.db", "accounts.key"] {
        std::fs::write(restored.path().join(name), entry(&mut zip, name)).unwrap();
    }
    let copy = AppDb::open(restored.path().join("app.db")).unwrap();
    assert_eq!(
        copy.accounts(copy.cipher()).get("mangadex").unwrap(),
        Some(account())
    );
    assert_eq!(
        copy.settings().get::<String>("general.language").unwrap(),
        Some("nl".into())
    );
}

#[tokio::test]
async fn an_unauthenticated_request_is_refused() {
    let h = harness();
    let state = h.state.clone().with_auth("hunter2");

    let res = get_backup(&state).await;

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
