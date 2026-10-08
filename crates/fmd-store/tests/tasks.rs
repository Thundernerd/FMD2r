use fmd_store::{AppDb, ChapterStatus, NewChapter, NewTask, PageStatus, TaskPage, TaskStatus};

fn new_task(title: &str, status: TaskStatus) -> NewTask {
    NewTask {
        module_id: "mod".into(),
        link: format!("/manga/{title}"),
        title: title.into(),
        save_to: "/downloads".into(),
        status,
        enabled: true,
    }
}

fn page(chapter_idx: u32, idx: u32) -> TaskPage {
    TaskPage {
        chapter_idx,
        idx,
        url: format!("https://img/{chapter_idx}/{idx}.jpg"),
        container_url: String::new(),
        filename: format!("{idx:03}"),
        status: PageStatus::Waiting,
    }
}

#[test]
fn task_with_chapters_and_pages_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let db = AppDb::open(&path).unwrap();
    let tasks = db.tasks();

    let task = tasks.create(&new_task("Berserk", TaskStatus::Waiting)).unwrap();
    let chapters: Vec<NewChapter> = (0..3)
        .map(|i| NewChapter {
            link: format!("/ch/{i}"),
            name: format!("Chapter {i}"),
            custom_filename: (i == 1).then(|| "special".to_string()),
        })
        .collect();
    tasks.set_chapters(task.id, &chapters).unwrap();
    // 10 pages: 4 + 3 + 3.
    for (chapter, count) in [(0, 4), (1, 3), (2, 3)] {
        let pages: Vec<_> = (0..count).map(|i| page(chapter, i)).collect();
        tasks.set_pages(task.id, chapter, &pages).unwrap();
    }
    let mut done = page(1, 2);
    done.status = PageStatus::Downloaded;
    done.filename = "002.jpg".into();
    tasks.update_page(task.id, &done).unwrap();
    drop(tasks);
    drop(db);

    let db = AppDb::open(&path).unwrap();
    let tasks = db.tasks();
    let reloaded = tasks.get(task.id).unwrap().unwrap();
    assert_eq!(reloaded, task);
    assert_eq!(reloaded.title, "Berserk");
    assert_eq!(reloaded.status, TaskStatus::Waiting);

    let chapters = tasks.chapters(task.id).unwrap();
    assert_eq!(chapters.len(), 3);
    assert_eq!(chapters[1].link, "/ch/1");
    assert_eq!(chapters[1].custom_filename.as_deref(), Some("special"));
    assert_eq!(chapters[0].status, ChapterStatus::Pending);
    assert_eq!(
        chapters.iter().map(|c| c.page_count).collect::<Vec<_>>(),
        [4, 3, 3]
    );

    let pages = tasks.pages(task.id, 1).unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[2], done);
    assert_eq!(pages[1].status, PageStatus::Waiting);
    let total: usize = (0..3).map(|c| tasks.pages(task.id, c).unwrap().len()).sum();
    assert_eq!(total, 10);
}

#[test]
fn list_by_status_filters_and_update_status_moves_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let tasks = db.tasks();
    let a = tasks.create(&new_task("a", TaskStatus::Downloading)).unwrap();
    let b = tasks.create(&new_task("b", TaskStatus::Stopped)).unwrap();
    let c = tasks.create(&new_task("c", TaskStatus::Downloading)).unwrap();

    let ids = |s| {
        tasks
            .list_by_status(s)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(TaskStatus::Downloading), [a.id, c.id]);

    tasks
        .update_status(a.id, TaskStatus::Failed, Some("timeout"))
        .unwrap();
    assert_eq!(ids(TaskStatus::Downloading), [c.id]);
    assert_eq!(ids(TaskStatus::Stopped), [b.id]);
    let failed = tasks.get(a.id).unwrap().unwrap();
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("timeout"));
}

#[test]
fn reorder_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let tasks = db.tasks();
    let a = tasks.create(&new_task("a", TaskStatus::Stopped)).unwrap();
    let b = tasks.create(&new_task("b", TaskStatus::Stopped)).unwrap();
    let c = tasks.create(&new_task("c", TaskStatus::Stopped)).unwrap();
    tasks
        .set_chapters(b.id, &[NewChapter {
            link: "/x".into(),
            name: "x".into(),
            custom_filename: None,
        }])
        .unwrap();
    tasks.set_pages(b.id, 0, &[page(0, 0)]).unwrap();

    tasks.reorder(&[c.id, a.id, b.id]).unwrap();
    let order: Vec<_> = tasks.list().unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(order, [c.id, a.id, b.id]);

    tasks.delete(b.id).unwrap();
    assert_eq!(tasks.get(b.id).unwrap(), None);
    assert!(tasks.chapters(b.id).unwrap().is_empty());
    assert!(tasks.pages(b.id, 0).unwrap().is_empty());
    assert_eq!(tasks.list().unwrap().len(), 2);
}
