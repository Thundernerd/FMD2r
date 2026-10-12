//! `metadata.db` through `MetadataBuilder` and `MetadataDb` (issue #237, "Seams under test").

// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use fmd_store::{MetadataBuilder, MetadataDb, MetadataSeries, StoreError};

fn build(path: &std::path::Path, series: &[MetadataSeries]) {
    let mut builder = MetadataBuilder::create(path).unwrap();
    for s in series {
        builder.add_series(s).unwrap();
    }
    builder.finish(1, "test").unwrap();
}

#[test]
fn an_empty_native_title_is_stored_as_null() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.db");
    build(
        &path,
        &[MetadataSeries {
            id: 1,
            title: "Blue".into(),
            native_title: Some(String::new()),
            romanized_title: Some("blue".into()),
            ..MetadataSeries::default()
        }],
    );

    let series = MetadataDb::open(&path).unwrap().series(1).unwrap().unwrap();
    assert_eq!(series.native_title, None);
    assert_eq!(series.romanized_title.as_deref(), Some("blue"));
}

#[test]
fn a_v1_file_reports_its_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("../src/migrations/metadata_v1.sql"))
        .unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    drop(conn);

    let err = MetadataDb::open(&path).err().unwrap();
    assert!(
        matches!(
            err,
            StoreError::SchemaMismatch {
                found: 1,
                supported: 3,
                ..
            }
        ),
        "{err:?}"
    );
}
