use fmd_store::{Account, AccountStatus, AppDb, KeyFileCipher};

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

#[test]
fn credentials_are_encrypted_at_rest_and_decrypted_through_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let cipher = KeyFileCipher::open_or_create(dir.path().join("secret.key")).unwrap();
    let db = AppDb::open(&path).unwrap();
    db.accounts(&cipher).upsert(&account()).unwrap();

    let raw = rusqlite::Connection::open(&path).unwrap();
    let (user, pass, cookies): (Vec<u8>, Vec<u8>, Vec<u8>) = raw
        .query_row(
            "SELECT username, password, cookies FROM accounts WHERE module_id = 'mangadex'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let dump = [user, pass, cookies].concat();
    for plain in ["reader", "hunter2", "topsecret"] {
        assert!(
            !dump.windows(plain.len()).any(|w| w == plain.as_bytes()),
            "{plain} stored in plaintext"
        );
    }

    assert_eq!(db.accounts(&cipher).get("mangadex").unwrap(), Some(account()));
    assert_eq!(db.accounts(&cipher).get("other").unwrap(), None);
}

#[test]
fn key_file_is_reused_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("secret.key");
    let path = dir.path().join("app.db");
    {
        let cipher = KeyFileCipher::open_or_create(&key).unwrap();
        AppDb::open(&path).unwrap().accounts(&cipher).upsert(&account()).unwrap();
    }
    let cipher = KeyFileCipher::open_or_create(&key).unwrap();
    let db = AppDb::open(&path).unwrap();
    assert_eq!(db.accounts(&cipher).get("mangadex").unwrap(), Some(account()));

    let other_dir = tempfile::tempdir().unwrap();
    let wrong = KeyFileCipher::open_or_create(other_dir.path().join("k")).unwrap();
    assert!(db.accounts(&wrong).get("mangadex").is_err());
}

#[test]
fn status_updates_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let cipher = KeyFileCipher::open_or_create(dir.path().join("secret.key")).unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let accounts = db.accounts(&cipher);
    accounts.upsert(&account()).unwrap();

    accounts.set_status("mangadex", AccountStatus::Invalid).unwrap();
    let stored = accounts.get("mangadex").unwrap().unwrap();
    assert_eq!(stored.status, AccountStatus::Invalid);
    assert_eq!(stored.password, "hunter2-secret");

    accounts.delete("mangadex").unwrap();
    assert_eq!(accounts.get("mangadex").unwrap(), None);
}
