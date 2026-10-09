//! Builds FMD2 userdata directories the way FMD2 writes them, and an FMD2r store to import into.

#![allow(dead_code, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use fmd_import::{ImportOptions, ImportReport, TimeZone};
use fmd_store::{AppDb, KeyFileCipher};
use rusqlite::{Connection, params};
use tempfile::TempDir;

/// `TStrings.Text` on Windows: every line followed by CRLF (`LineEnding`).
pub fn text(lines: &[&str]) -> String {
    lines.iter().map(|l| format!("{l}\r\n")).collect()
}

/// An FMD2 `userdata` directory (baseunits/FMDOptions.pas:286-295).
pub struct Fmd2 {
    dir: TempDir,
}

/// One row of FMD2's `downloads` table, in `TDownloadsDB.Add`'s column order
/// (baseunits/DownloadsDB.pas:96-131).
pub struct DownloadRow {
    pub enabled: bool,
    pub order: i64,
    pub taskstatus: i64,
    pub chapterptr: i64,
    pub numberofpages: i64,
    pub currentpage: i64,
    pub moduleid: &'static str,
    pub link: &'static str,
    pub title: &'static str,
    pub status: &'static str,
    pub progress: &'static str,
    pub saveto: &'static str,
    pub dateadded: &'static str,
    pub datelastdownloaded: &'static str,
    pub chapterslinks: String,
    pub chaptersnames: String,
    pub pagelinks: String,
    pub pagecontainerlinks: String,
    pub filenames: String,
    pub customfilenames: &'static str,
    pub chaptersstatus: String,
}

impl Default for DownloadRow {
    fn default() -> Self {
        Self {
            enabled: true,
            order: 0,
            taskstatus: 0,
            chapterptr: 0,
            numberofpages: 0,
            currentpage: 0,
            moduleid: "46e0c618a19748d6af150c2f198f5360",
            link: "/manga/berserk",
            title: "Berserk",
            status: "",
            progress: "",
            saveto: "/manga/Berserk",
            dateadded: "2024-03-05 14:07:09.123",
            datelastdownloaded: "2024-03-06 08:00:00.000",
            chapterslinks: String::new(),
            chaptersnames: String::new(),
            pagelinks: String::new(),
            pagecontainerlinks: String::new(),
            filenames: String::new(),
            customfilenames: "%FILENAME%",
            chaptersstatus: String::new(),
        }
    }
}

/// One row of FMD2's `favorites` table (baseunits/FavoritesDB.pas:73-95).
pub struct FavoriteRow {
    pub id: &'static str,
    pub order: i64,
    pub enabled: bool,
    pub moduleid: &'static str,
    pub link: &'static str,
    pub title: &'static str,
    pub status: &'static str,
    pub currentchapter: &'static str,
    pub downloadedchapterlist: String,
    pub saveto: &'static str,
    pub dateadded: &'static str,
    pub datelastchecked: &'static str,
    pub datelastupdated: &'static str,
}

impl Default for FavoriteRow {
    fn default() -> Self {
        Self {
            id: "46e0c618a19748d6af150c2f198f5360/manga/berserk",
            order: 0,
            enabled: true,
            moduleid: "46e0c618a19748d6af150c2f198f5360",
            link: "/manga/berserk",
            title: "Berserk",
            status: "1",
            currentchapter: "0",
            downloadedchapterlist: String::new(),
            saveto: "/manga/Berserk",
            dateadded: "2023-01-02 03:04:05.006",
            datelastchecked: "2023-02-01 00:00:00.000",
            datelastupdated: "2023-02-02 00:00:00.000",
        }
    }
}

impl Fmd2 {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn db(&self, name: &str, create: &str) -> Connection {
        let conn = Connection::open(self.dir().join(name)).unwrap();
        conn.execute_batch(create).unwrap();
        conn
    }

    /// `downloads.db` with `TDownloadsDB`'s table (baseunits/DownloadsDB.pas:67-89, created by
    /// SQLiteData.pas:462).
    pub fn downloads(&self, rows: &[DownloadRow]) {
        let conn = self.db(
            "downloads.db",
            r#"CREATE TABLE "downloads" ("id" INTEGER PRIMARY KEY,"enabled" BOOLEAN,"order" INTEGER,"taskstatus" INTEGER,"chapterptr" INTEGER,"numberofpages" INTEGER,"currentpage" INTEGER,"moduleid" TEXT,"link" TEXT,"title" TEXT,"status" TEXT,"progress" TEXT,"saveto" TEXT,"dateadded" DATETIME,"datelastdownloaded" DATETIME,"chapterslinks" TEXT,"chaptersnames" TEXT,"pagelinks" TEXT,"pagecontainerlinks" TEXT,"filenames" TEXT,"customfilenames" TEXT,"chaptersstatus" TEXT)"#,
        );
        for r in rows {
            conn.execute(
                r#"INSERT INTO "downloads" ("enabled","order","taskstatus","chapterptr","numberofpages","currentpage","moduleid","link","title","status","progress","saveto","dateadded","datelastdownloaded","chapterslinks","chaptersnames","pagelinks","pagecontainerlinks","filenames","customfilenames","chaptersstatus") VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)"#,
                params![
                    if r.enabled { "1" } else { "0" },
                    r.order,
                    r.taskstatus,
                    r.chapterptr,
                    r.numberofpages,
                    r.currentpage,
                    r.moduleid,
                    r.link,
                    r.title,
                    r.status,
                    r.progress,
                    r.saveto,
                    r.dateadded,
                    r.datelastdownloaded,
                    r.chapterslinks,
                    r.chaptersnames,
                    r.pagelinks,
                    r.pagecontainerlinks,
                    r.filenames,
                    r.customfilenames,
                    r.chaptersstatus
                ],
            )
            .unwrap();
        }
    }

    /// `favorites.db` with `TFavoritesDB`'s table (baseunits/FavoritesDB.pas:55-68).
    pub fn favorites(&self, rows: &[FavoriteRow]) {
        let conn = self.db(
            "favorites.db",
            r#"CREATE TABLE "favorites" ("id" VARCHAR(3000) NOT NULL PRIMARY KEY,"order" INTEGER,"enabled" BOOLEAN,"moduleid" TEXT,"link" TEXT,"title" TEXT,"status" TEXT,"currentchapter" TEXT,"downloadedchapterlist" TEXT,"saveto" TEXT,"dateadded" DATETIME,"datelastchecked" DATETIME,"datelastupdated" DATETIME)"#,
        );
        for r in rows {
            conn.execute(
                r#"INSERT OR REPLACE INTO "favorites" ("id","order","enabled","moduleid","link","title","status","currentchapter","downloadedchapterlist","saveto","dateadded","datelastchecked","datelastupdated") VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"#,
                params![
                    r.id,
                    r.order,
                    if r.enabled { "1" } else { "0" },
                    r.moduleid,
                    r.link,
                    r.title,
                    r.status,
                    r.currentchapter,
                    r.downloadedchapterlist,
                    r.saveto,
                    r.dateadded,
                    r.datelastchecked,
                    r.datelastupdated
                ],
            )
            .unwrap();
        }
    }

    /// `downloadedchapters.db` with `TDownloadedChaptersDB`'s table
    /// (baseunits/DownloadedChaptersDB.pas:124-127); `id` is `LowerCase(ModuleID+Link)` (:65).
    pub fn downloaded_chapters(&self, rows: &[(&str, String)]) {
        let conn = self.db(
            "downloadedchapters.db",
            r#"CREATE TABLE "downloadedchapters" ("id" VARCHAR(3000) NOT NULL PRIMARY KEY,"chapters" TEXT)"#,
        );
        for (id, chapters) in rows {
            conn.execute(
                r#"INSERT INTO "downloadedchapters" ("id","chapters") VALUES (?1,?2)"#,
                params![id, chapters],
            )
            .unwrap();
        }
    }

    pub fn file(&self, name: &str, contents: &str) {
        std::fs::write(self.dir().join(name), contents).unwrap();
    }
}

/// An FMD2r data directory with `app.db` and an account key file.
pub struct App {
    dir: TempDir,
    pub db: AppDb,
    pub cipher: KeyFileCipher,
}

impl App {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let cipher = KeyFileCipher::open_or_create(dir.path().join("accounts.key")).unwrap();
        Self { dir, db, cipher }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Imports with FMD2's timestamps read as UTC.
    pub fn import(&self, fmd2: &Fmd2) -> ImportReport {
        self.import_with(
            fmd2,
            &ImportOptions {
                timezone: TimeZone::UTC,
                ..ImportOptions::default()
            },
        )
    }

    pub fn import_with(&self, fmd2: &Fmd2, opts: &ImportOptions) -> ImportReport {
        fmd_import::import(fmd2.dir(), &self.db, &self.cipher, opts).unwrap()
    }
}

/// A userdata directory with every source: two tasks, a favorite with a downloaded chapter, a
/// downloaded-chapters row, a module with settings and an account, and two settings.
pub fn every_source() -> Fmd2 {
    let fmd2 = Fmd2::new();
    fmd2.downloads(&[
        DownloadRow {
            chapterslinks: text(&["/ch/1"]),
            chaptersnames: text(&["Ch. 1"]),
            ..DownloadRow::default()
        },
        DownloadRow {
            order: 1,
            link: "/manga/guts",
            title: "Guts",
            saveto: "C:\\Manga\\Guts",
            ..DownloadRow::default()
        },
    ]);
    fmd2.favorites(&[FavoriteRow {
        downloadedchapterlist: text(&["/ch/1"]),
        ..FavoriteRow::default()
    }]);
    fmd2.downloaded_chapters(&[(
        "46e0c618a19748d6af150c2f198f5360/manga/other",
        text(&["/ch/9"]),
    )]);
    fmd2.file(
        "modules.json",
        r#"[{"ID":"d07c9c2425764da8ba056505f57cf40c",
            "Settings":{"Enabled":true,"MaxTaskLimit":1},
            "Options":{"lang":1},
            "Account":{"Enabled":true,"Username":"","Password":"pcjhSQpguA==","Status":"asValid","Cookies":""},
            "Cookies":[]}]"#,
    );
    fmd2.file(
        "settings.json",
        r#"{"connections":{"NumberOfTasks":3},"saveto":{"SaveTo":"C:\\Manga"}}"#,
    );
    fmd2
}
