use fmd_store::AppDb;

#[test]
fn mark_then_contains_is_idempotent_and_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let repo = db.downloaded_chapters();

    assert!(!repo.contains("mod", "/manga/1", "/ch/1").unwrap());
    repo.mark("mod", "/manga/1", &["/ch/1", "/ch/2"]).unwrap();
    repo.mark("mod", "/manga/1", &["/ch/1"]).unwrap();

    assert!(repo.contains("mod", "/manga/1", "/ch/1").unwrap());
    // FMD2 keys and merges case-insensitively (baseunits/DownloadedChaptersDB.pas:70-73).
    assert!(repo.contains("MOD", "/Manga/1", "/CH/2").unwrap());
    assert!(!repo.contains("mod", "/manga/2", "/ch/1").unwrap());
    assert_eq!(repo.list_for("mod", "/manga/1").unwrap(), ["/ch/1", "/ch/2"]);
}
