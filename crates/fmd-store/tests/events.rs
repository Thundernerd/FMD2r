use fmd_store::{AppDb, EventQuery, EventSeverity, NewEvent};
use serde_json::json;

fn event(title: &str, severity: EventSeverity) -> NewEvent {
    NewEvent {
        kind: "module_update".into(),
        severity,
        module_id: Some("mangadex".into()),
        task_id: None,
        title: title.into(),
        body: json!({ "files": 3 }),
    }
}

#[test]
fn push_list_and_mark_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let events = db.events();

    let first = events.push(&event("first", EventSeverity::Info)).unwrap();
    let second = events.push(&event("second", EventSeverity::Error)).unwrap();
    assert!(!first.read);
    assert_eq!(second.body, json!({ "files": 3 }));
    assert_eq!(second.severity, EventSeverity::Error);
    assert_eq!(second.module_id.as_deref(), Some("mangadex"));

    let all = events.list(&EventQuery::default()).unwrap();
    assert_eq!(all, [second.clone(), first.clone()], "newest first");

    events.mark_read(&[first.id]).unwrap();
    let unread = events
        .list(&EventQuery {
            unread_only: true,
            ..EventQuery::default()
        })
        .unwrap();
    assert_eq!(unread, [second.clone()]);
    assert_eq!(events.unread_count().unwrap(), 1);

    let limited = events
        .list(&EventQuery {
            limit: Some(1),
            ..EventQuery::default()
        })
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].id, second.id);

    events.mark_all_read().unwrap();
    assert_eq!(events.unread_count().unwrap(), 0);
}
