// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, DownloadRow, Fmd2, text};
use fmd_store::{ChapterStatus, PageStatus, TaskStatus};

#[test]
fn a_download_becomes_a_task_with_its_chapters_and_the_current_chapters_pages() {
    let fmd2 = Fmd2::new();
    // FMD2 writes the joined fields with `TStrings.Text` (baseunits/uDownloadsManager.pas:1464-1470);
    // the pages belong to the chapter at `chapterptr`, and 'D'/'W' replace a page's link once it
    // is downloaded or waiting for its URL (:353, :407).
    fmd2.downloads(&[DownloadRow {
        taskstatus: 0,
        chapterptr: 1,
        numberofpages: 3,
        currentpage: 2,
        chapterslinks: text(&["/ch/1", "/ch/2", "/ch/3"]),
        chaptersnames: text(&["Ch. 1", "Ch. 2", "Ch. 3"]),
        pagelinks: text(&["D", "https://img/2/2.jpg", "W"]),
        pagecontainerlinks: text(&["/ch/2/p1", "/ch/2/p2", "/ch/2/p3"]),
        filenames: text(&["001", "002", "003"]),
        chaptersstatus: text(&["D", "F", "P"]),
        ..DownloadRow::default()
    }]);
    let app = App::new();

    let report = app.import(&fmd2);

    assert_eq!(report.tasks.imported, 1);
    let tasks = app.db.tasks().list().unwrap();
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.module_id, "46e0c618a19748d6af150c2f198f5360");
    assert_eq!(task.link, "/manga/berserk");
    assert_eq!(task.title, "Berserk");
    assert_eq!(task.save_to, "/manga/Berserk");
    assert_eq!(task.status, TaskStatus::Stopped);
    assert!(task.enabled);
    assert_eq!(task.current_chapter, 1);
    // 'YYYY-MM-DD hh:nn:ss.zzz' (baseunits/SQLiteData.pas:159-167), read as UTC.
    assert_eq!(task.date_added, 1_709_647_629_123);
    assert_eq!(task.date_last_downloaded, Some(1_709_712_000_000));

    let chapters = app.db.tasks().chapters(task.id).unwrap();
    let summary: Vec<_> = chapters
        .iter()
        .map(|c| (c.link.as_str(), c.name.as_str(), c.status, c.current_page))
        .collect();
    assert_eq!(
        summary,
        [
            ("/ch/1", "Ch. 1", ChapterStatus::Downloaded, 0),
            ("/ch/2", "Ch. 2", ChapterStatus::Failed, 2),
            ("/ch/3", "Ch. 3", ChapterStatus::Pending, 0),
        ]
    );
    assert!(
        chapters
            .iter()
            .all(|c| c.custom_filename.as_deref() == Some("%FILENAME%"))
    );

    let pages = app.db.tasks().pages(task.id, 1).unwrap();
    let pages: Vec<_> = pages
        .iter()
        .map(|p| {
            (
                p.url.as_str(),
                p.container_url.as_str(),
                p.filename.as_str(),
                p.status,
            )
        })
        .collect();
    assert_eq!(
        pages,
        [
            ("", "/ch/2/p1", "001", PageStatus::Downloaded),
            (
                "https://img/2/2.jpg",
                "/ch/2/p2",
                "002",
                PageStatus::Waiting
            ),
            ("", "/ch/2/p3", "003", PageStatus::Waiting),
        ]
    );
    assert!(app.db.tasks().pages(task.id, 0).unwrap().is_empty());
}

/// One task per `TDownloadStatusType` ordinal (baseunits/uDownloadsManager.pas:19-31), in order.
fn one_task_per_status(fmd2: &Fmd2) {
    let rows: Vec<DownloadRow> = (0..10)
        .map(|status| DownloadRow {
            order: status,
            taskstatus: status,
            dateadded: [
                "2024-01-01 00:00:00.000",
                "2024-01-01 00:00:01.000",
                "2024-01-01 00:00:02.000",
                "2024-01-01 00:00:03.000",
                "2024-01-01 00:00:04.000",
                "2024-01-01 00:00:05.000",
                "2024-01-01 00:00:06.000",
                "2024-01-01 00:00:07.000",
                "2024-01-01 00:00:08.000",
                "2024-01-01 00:00:09.000",
            ][status as usize],
            ..DownloadRow::default()
        })
        .collect();
    fmd2.downloads(&rows);
}

fn statuses(app: &App) -> Vec<TaskStatus> {
    app.db
        .tasks()
        .list()
        .unwrap()
        .iter()
        .map(|t| t.status)
        .collect()
}

#[test]
fn task_statuses_map_from_fmd2_ordinals_with_running_tasks_stopped() {
    let fmd2 = Fmd2::new();
    one_task_per_status(&fmd2);
    let app = App::new();

    app.import(&fmd2);

    use TaskStatus::*;
    assert_eq!(
        statuses(&app),
        // STOP, WAIT, PREPARE, DOWNLOAD, FINISH, CONVERT, COMPRESS, PROBLEM, FAILED, NONE
        [
            Stopped, Stopped, Stopped, Stopped, Finished, Stopped, Stopped, Failed, Failed, Stopped
        ]
    );
}

#[test]
fn running_tasks_can_be_imported_as_waiting_to_resume() {
    let fmd2 = Fmd2::new();
    one_task_per_status(&fmd2);
    let app = App::new();

    app.import_with(
        &fmd2,
        &fmd_import::ImportOptions {
            resume_in_progress: true,
            ..Default::default()
        },
    );

    use TaskStatus::*;
    assert_eq!(
        statuses(&app),
        [
            Stopped, Waiting, Waiting, Waiting, Finished, Waiting, Waiting, Failed, Failed, Stopped
        ]
    );
}

#[test]
fn tasks_keep_fmd2s_queue_order_and_enabled_flag() {
    let fmd2 = Fmd2::new();
    fmd2.downloads(&[
        DownloadRow {
            order: 1,
            title: "Second",
            link: "/b",
            enabled: false,
            ..DownloadRow::default()
        },
        DownloadRow {
            order: 0,
            title: "First",
            link: "/a",
            ..DownloadRow::default()
        },
    ]);
    let app = App::new();

    app.import(&fmd2);

    let tasks: Vec<_> = app
        .db
        .tasks()
        .list()
        .unwrap()
        .into_iter()
        .map(|t| (t.title, t.enabled))
        .collect();
    assert_eq!(
        tasks,
        [("First".to_string(), true), ("Second".to_string(), false)]
    );
}
