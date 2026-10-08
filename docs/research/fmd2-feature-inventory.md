# FMD2 feature inventory for FMD2r v1

Ticket: [#2 Inventory FMD2 features for v1](https://github.com/Thundernerd/FMD2r/issues/2) (map: #1).

**Question:** What is the complete list of user-facing features and options in FMD2 (every tab, dialog, option page, setting and background behaviour), and how is each one tagged for v1?

**Source:** the upstream FMD2 checkout at `~/Repositories/Forks/FMD2`, baseline commit `ad3a5b63`. Paths below are relative to that checkout. `frmMain.pas` and `frmMain.lfm` mean `mangadownloader/forms/frmMain.{pas,lfm}`. Line numbers are at `ad3a5b63`.

## Tags

The tags follow the standing decisions in map #1. FMD2r is a headless, single-user, self-hosted server. Desktop-only features get server equivalents. Power actions are dropped. App self-update is replaced by image/release updates. The UI is English only, with strings kept translatable.

| Tag | Meaning |
|---|---|
| **PORT** | Port as-is. Same behaviour, with a web control in place of the desktop control. |
| **SERVER: …** | Server equivalent. The text after the colon names the replacement. |
| **DROP** | Not in v1. The reason is given. |

"Parity checklist" means the PORT and SERVER rows. The UI prototype (#13) and the storage design (#15) must cover those rows.

## Summary

- FMD2 has 5 top-level tabs: Downloads, Manga Info, Favorites, Options and About. Options has 8 pages. The Websites page has 6 sub-tabs, and Misc has 2. All of these are listed below.
- `userdata/settings.json` holds about 95 user-editable keys in 16 sections, plus window and column state. Per-module state lives in `userdata/modules.json`: settings, module options, the account (encrypted) and cookies. Distribution config lives in `config.json`.
- Background behaviour: startup sequence, favorites check on startup and on an interval, auto-download after a check, auto-removal of completed series, module-update checks, app-update checks, download queue scheduling with retries, a periodic DB flush, a daily settings backup, and "after download finish" power actions.
- Almost all of the core features are PORT. The desktop shell maps to SERVER equivalents: tray, drop box, open folder or external program, balloon hints, window state, single instance, self-update and the log window. DROP covers power actions, the language picker, custom list colours, Windows-only path and standby tweaks, toolbar-visibility toggles and the exit confirmation.
- The source has several bugs and dead options (see [Surprises](#surprises-and-upstream-bugs)). They matter for the FMD2 importer and for deciding what "parity" means.

---

## 1. Main window

The main window is `MainForm` with page control `pcMain` (`frmMain.lfm:16`). It has a status bar `sbMain` (`frmMain.lfm:5507`). The status bar shows the progress of background "silent" jobs, and its context menu has an **Abort** item (`pmSbMain`, `frmMain.lfm:6475`).

| Feature | Source | Tag |
|---|---|---|
| Status bar with background-job progress ("Loading: %d/%d") and Abort | `frmMain.lfm:5507,6475`; `baseunits/uSilentThread.pas:105` | **SERVER: activity/jobs indicator in the web UI with cancel** |
| Tray icon, tray menu (Resume all, Stop all, After download finish ▸ Nothing/Exit/Shutdown/Hibernate, Show Drop Box, Restore, Exit) | `frmMain.lfm:5822,6500-6559` | **DROP** (no tray on a server). Resume all and Stop all already exist on the Downloads tab. |
| Window position, size, maximized state, splitter positions, last active tab | `frmMain.pas` `LoadFormInformation`/`SaveFormInformation` (6680, 6792); keys under `form` | **SERVER: client-side UI state (browser localStorage)** |
| Per-list sort column, direction, column widths and order (`vtDownload`, `vtFavorites`, …) | `frmMain.pas:6680+` (keys `<treename>/SortColumn`, `SortDirection`, `Column<i>Width`, `Column<i>Position`) | **SERVER: client-side UI state.** Download/favorite sort order that affects queue order stays server-side. |
| Keyboard: Ctrl+Up/Down/Home/End moves download tasks; Delete deletes; Space toggles chapters; Ctrl+Shift+V paste-and-go and Ctrl+Shift+C copy the URL | `frmMain.pas:3192,4794-4819,6609-6622` | **PORT** (as web shortcuts) |

## 2. Downloads tab (`tsDownload`)

Source: `frmMain.lfm:27-543`; task logic in `baseunits/uDownloadsManager.pas`.

| Feature | Source | Tag |
|---|---|---|
| Download task list. Columns: Manga, Status, Progress, Transfer rate, Website, Save to, Date added, Last downloaded date | `frmMain.lfm:456-507` | **PORT** |
| Filter tree, by status: All downloads ▸ Completed / In progress / Stopped / Failed / Disabled | `frmMain.pas:2602-2644` | **PORT** |
| Filter tree, by history: Today / Yesterday / Last 7 days / This month / Last 6 months / Older than 6 months / Custom (from–to date range + Apply) | `frmMain.pas:2634-2642`; `frmMain.lfm:74-142`; keys `general/DownloadFilterSelect`, `DownloadFilter/CustomFrom`, `DownloadFilter/CustomTo` | **PORT** (the last-selected filter becomes client-side UI state) |
| Search downloads | `frmMain.lfm:341` | **PORT** |
| Live transfer-rate graph (shown while tasks run) | `frmMain.lfm:206`; `frmMain.pas:1995-2028` | **PORT** |
| Toolbar: Resume All, Stop All, Delete all completed tasks | `frmMain.lfm:307-334` | **PORT** |
| Left toolbar: Move Top / Up / Down / Bottom. Also drag-and-drop reordering. | `frmMain.lfm:423-447`; `frmMain.pas:4603-4690` | **PORT** |
| Context menu: Stop, Resume, Redownload, Enable, Disable | `frmMain.lfm:5538-5560` | **PORT** |
| Context menu: Delete ▸ Task only / Task + Data / Task + Data + Favorite | `frmMain.lfm:5565-5580`; `frmMain.pas:2214` | **PORT** |
| Context menu: Delete all completed tasks | `frmMain.lfm:5584` | **PORT** |
| Context menu: Merge completed tasks (finished tasks with the same title, module and save-to are merged into one) | `frmMain.pas:2346-2383` | **PORT** |
| Context menu: View manga info | `frmMain.lfm:5601` | **PORT** |
| Context menu: Open Folder | `frmMain.lfm:5607`; `frmMain.pas:4153` | **SERVER: download the chapter/archive to the browser, or browse the files** |
| Context menu: Open ... (with external program) | `frmMain.lfm:5613`; `frmMain.pas:6972-7060` | **SERVER: file download (same as Open Folder)** |
| Hover hint with task details | `frmMain.pas:4705` | **PORT** (as a tooltip or details panel) |

## 3. Manga Info tab (`tsInformation`)

Source: `frmMain.lfm:544-2457`. This tab is the browse, search and download entry point.

### 3a. Manga list (left pane)

| Feature | Source | Tag |
|---|---|---|
| Website selector. It lists only the websites ticked in Options ▸ Websites, and the selection is stored as `general/MangaListSelect` (comma-separated module IDs). | `frmMain.lfm:605`; `frmMain.pas:5990-6005` | **PORT** |
| Update-list menu: Update manga list; Download manga list from FMD server; Update all lists at once; Download all lists from FMD server at once | `frmMain.lfm:5832-5852`; `baseunits/uUpdateThread.pas`; `baseunits/DBUpdater.pas` | **PORT** (the server list is FMD2-DB, as the map decides) |
| Manga list for the selected website: title search box with clear button, mode label "Show all (n)" / "Filtered (n)", Remove-filter button | `frmMain.lfm:566-807`; `frmMain.pas:1000-1001` | **PORT** |
| Hover hint with manga metadata | `frmMain.pas:5161` | **PORT** |
| Highlight titles that are new within N days (`update/NewMangaTime`) | `frmMain.pas:5132` | **PORT** |
| Context: View manga infos, Download all, Add to Favorites, Delete (removes the entry from the local list DB), Highlight new manga (toggle) | `frmMain.lfm:5787-5818` | **PORT** |

### 3b. Info sub-tab (`tsInfoManga`)

| Feature | Source | Tag |
|---|---|---|
| URL box. Paste a URL, the module is resolved by host, and the manga info loads. "URL not supported!" if no module matches. Has a context menu (undo, cut, copy, paste, **Paste and go**, delete, select all). | `frmMain.lfm:860,6413-6448`; `frmMain.pas:6579-6607` | **PORT** (normal text-field editing is native in a browser) |
| Info panel: cover image, Website, Title, Alternative titles, Author(s), Artist(s), Genre(s), Status, Summary | `frmMain.lfm:982`; `frmMain.pas:1003-1011,5724` | **PORT** |
| Chapter list with checkboxes, highlighting of downloaded chapters and a filter box | `frmMain.lfm:1025,1291` | **PORT** |
| Chapter context menu: Check/Uncheck selected, Check/Uncheck all, Highlight downloaded chapters, Hide downloaded chapters, Filter, Ascending/Descending | `frmMain.lfm:5620-5671`; keys `general/HighlightDownloadedChapters`, `ChapterListHideDownloaded`, `SortChapterListAscending` | **PORT** |
| Download button | `frmMain.pas:2646-2795` | **PORT** |
| Split download: split the checked chapters into N tasks ("Download count:") | `frmMain.pas:973-974,2712-2780,3100` | **PORT** |
| "Add to download list as stopped task" checkbox (`general/AddAsStopped`) | `frmMain.lfm:1110` | **PORT** |
| Read online (opens the manga URL in the system browser) | `frmMain.lfm:1127`; `frmMain.pas:3060` | **PORT** (as an external link) |
| Add to favorites | `frmMain.pas:2797-2846` | **PORT** |
| Per-download "Save to" path. The default comes from the global path or the module's `OverrideSettings.SaveToPath`. | `frmMain.lfm:1342`; `frmMain.pas:5616-5644` | **PORT** (the path is inside the server's download volume) |
| "Title already in download list, download anyway?" prompt | `frmMain.pas:957` | **PORT** |

### 3c. Filter sub-tab (`tsinfoFilterAdv`)

Search over the local manga-list DB.

| Feature | Source | Tag |
|---|---|---|
| Text filters: Title, Author, Artist, Summary; Status (Completed/Ongoing/Hiatus/Cancelled/&lt;none&gt;) | `frmMain.lfm:1557-1665` | **PORT** |
| 37 fixed genre tri-state checkboxes (Action … Webtoons), with a menu to check all, uncheck all or set all indeterminate | `frmMain.lfm:1750-2276,6484-6496` | **PORT** |
| Custom genres (comma-separated) | `frmMain.lfm:1498` | **PORT** |
| Match one vs. all genres; Search only new manga; Search in all manga sites; Regular expression | `frmMain.lfm:1684-1726` | **PORT** |
| Filter / Remove filter / Reset value | `frmMain.lfm:2302-2408`; `frmMain.pas:3338-3460` | **PORT** |

## 4. Favorites tab (`tsFavorites`)

Source: `frmMain.lfm:2458-2796`; logic in `baseunits/uFavoritesManager.pas`.

| Feature | Source | Tag |
|---|---|---|
| Favorites list. Columns: #, Title, Current chapter, Website, Status, Save to, Date added, Last checked date, Last updated date | `frmMain.lfm:2484-2520` | **PORT** |
| Row states (coloured): broken/problem ("removing and re-adding may fix"), checking, new chapter found, completed, empty chapters | `baseunits/FMDOptions.pas` `CL_FV*`; `frmMain.pas:955` | **PORT** (as status badges, not custom colours) |
| Show All / Enabled / Disabled; Search favorites | `frmMain.lfm:2704-2792` | **PORT** |
| Check for new chapter button, with Cancel | `frmMain.lfm:2601-2664` | **PORT** |
| Import list (dialog, see §6) | `frmMain.lfm:2549` | **PORT** (FMD-format part; see §6) |
| Context: Check for new chapter, **Check for missing chapters** (scans the save folder for each chapter as dir/cbz/zip/pdf/epub and queues the ones missing on disk), Stop check | `frmMain.lfm:5682-5692`; `uFavoritesManager.pas:397+,883` | **PORT** |
| Context: Enable, Disable (disabled favorites are skipped by checks) | `frmMain.lfm:5697-5701`; `uFavoritesManager.pas:832-882` | **PORT** |
| Context: View manga info, Download all | `frmMain.lfm:5705-5711` | **PORT** |
| Context: Rename (edits the favorite's title) | `frmMain.pas:1653-1670` | **PORT** |
| Context: Transfer website (dialog, see §6) | `frmMain.lfm:5720` | **PORT** |
| Context: Delete (with confirmation) | `frmMain.lfm:5727` | **PORT** |
| Context: Change "Save to". Sets a new path and offers to move existing files. For multiple favorites, the new path is `{New_Path}/{Manga_Title}`; if only the drive letter changes, only the letter is replaced. | `frmMain.pas:963-964,3582`; `frmSelectDirectory.lfm` | **PORT** (the drive-letter special case is Windows-only and is dropped) |
| Context: Open Folder, Open ... (external program) | `frmMain.lfm:5740-5746` | **SERVER: file download/browse** |
| Context: Default Action on double-click: Open Folder / View Manga Info / Rename / Check for new Chapters (`favorites/DefaultAction` 0–3) | `frmMain.lfm:5754-5780`; `frmMain.pas:5844-5851` | **PORT** (open folder becomes the file-browse equivalent) |
| Drag-and-drop of URLs onto the favorites list | `frmMain.pas:4924-4938` | **SERVER: "add by URL" input (see Drop Box)** |

## 5. Options tab (`tsOption`)

Changes apply only when **Apply** is pressed (`btOptionApply`, `frmMain.pas:5093`). `LoadOptions` and `SaveOptions` (`frmMain.pas:5803-6167`) are the authoritative key list. Keys are `section/Key` in `userdata/settings.json`. Defaults are the ones `LoadOptions` reads.

### 5.1 General (`tsGeneral`)

| Option | Key (default) | Tag |
|---|---|---|
| Language | `languages/Selected` ("en") | **DROP** (English only in v1; strings stay translatable; `fmd.env.SelectedLanguage` still answers) |
| After download finish: Nothing / Exit FMD / Shutdown / Hibernate | `general/LetFMDDo` (0) | **DROP** power actions and exit. **SERVER: "all downloads finished" notification event.** |
| Theme: System / Dark / Light (needs a restart) | `darkmode/mode` (0) | **PORT** (live switch, no restart) |
| New manga based on update time (days) | `update/NewMangaTime` (1) | **PORT** |
| Minimize on start | `general/MinimizeOnStart` (false) | **DROP** |
| Minimize to tray | `general/MinimizeToTray` (false) | **DROP** |
| Permit only one FMD running | `general/OneInstanceOnly` (true) | **SERVER: always-on exclusive lock on the data directory** (not a setting) |
| Enable live search (slow on long list) | `general/LiveSearch` (true) | **PORT** (search-as-you-type vs. search on Enter) |
| Delete completed tasks on close | `general/DeleteCompletedTasksOnClose` (false) | **SERVER: auto-clear completed tasks** (on server start, or after a retention period) |
| Sort Downloads when adding new tasks | `general/SortDownloadsOnNewTasks` (false) | **PORT** |
| Vacuum databases on exit | `general/VacuumDatabasesOnExit` (false) | **SERVER: scheduled/maintenance DB compaction** (depends on the #15 storage choice) |
| Enable long name paths (Windows) | `general/EnableLongNamePaths` (false) | **DROP** (Windows MAX_PATH workaround; Linux is the target) |
| External program path + parameters (`%PATH%`, `%CHAPTER%`; default `"%PATH%%CHAPTER%"`) | `general/ExternalProgramPath` (""), `general/ExternalProgramParams` | **SERVER: file download** (the map's "file download instead of open folder") |

### 5.2 View (`tsView`)

| Option | Key (default) | Tag |
|---|---|---|
| Show downloads toolbar | `view/ShowDownloadsToolbar` (true) | **DROP** (the web layout decides; the actions stay) |
| Show left downloads toolbar | `view/ShowDownloadsToolbarLeft` (true) | **DROP** |
| Show "Delete all completed tasks" in the downloads toolbar | `view/ShowDownloadsToolbarDeleteAll` (false) | **DROP** |
| Enable load manga cover | `view/LoadMangaCover` (true) | **PORT** |
| Show balloon hint (tray toast when a task finishes or fails) | `view/ShowBalloonHint` (true) | **SERVER: in-UI toast + notification event** (targets decided in the notifications ticket) |
| Show Downloads tab when adding new tasks | `view/ShowDownloadsTabOnNewTasks` (true) | **PORT** |
| Show Favorites tab when adding new manga | `view/ShowFavoritesTabOnNewManga` (false) | **PORT** |
| Drop Box: show, opacity, mode (Download all / Add to favorites), geometry. A floating always-on-top window that accepts dragged URLs. | `droptarget/Show,Mode,Opacity,Width,Heigth,Top,Left`; `frmDropTarget.*` | **SERVER: "Add by URL" box with a mode choice (download all / add to favorites)**; geometry and opacity are dropped |

### 5.3 Connections (`tsConnections`)

| Option | Key (default) | Tag |
|---|---|---|
| Connection timeout (seconds) | `connections/ConnectionTimeout` (30) | **PORT** |
| Max retries on connection failure (-1 = always) | `connections/Retry` (5) | **PORT** |
| Default user-agent | `connections/DefaultUserAgent` ("" = built-in) | **PORT** |
| Use proxy; type HTTP/SOCKS4/SOCKS5; host; port; username; password (user and password stored with `EncryptString`) | `connections/UseProxy,ProxyType,Host,Port,User,Pass` | **PORT** |
| Number of downloaded tasks at the same time | `connections/NumberOfTasks` (1). Max 8 on Linux, 16 on win32, 64 on win64 (`FMDOptions.pas:36-48`) | **PORT** (choose a server ceiling; #11) |
| Number of downloaded files per task at the same time | `connections/NumberOfThreadsPerTask` (1). Max 32 on Linux, 256 on win64 | **PORT** |
| Number of retries if a task failed | `connections/NumberOfAutoRetryFailedTask` (1) | **PORT** |
| Always start task from failed chapters | read `connections/AlwaysStartFromFailedChapters` (true), written as `AlwaysRetruFailedChaptersOnStart` (see Surprises) | **PORT** |
| Max number of favorite checks at the same time | `connections/MaxFavoriteThreads` (1) | **PORT** |
| Max number of update-list threads | `connections/MaxUpdateListThreads` (1) | **PORT** |
| Max number of background load threads (Download all / Add to favorites jobs) | `connections/MaxBackgroundLoadThreads` (1) | **PORT** |

### 5.4 Save to (`tsSaveTo`)

| Option | Key (default) | Tag |
|---|---|---|
| Default download path | `saveto/SaveTo` (`downloads/`) | **PORT** (relative to or inside the container volume) |
| Save downloaded chapters as: None (folder) / ZIP / CBZ / PDF / EPUB | `saveto/Compress` (0) | **PORT** (fidelity per the image-pipeline ticket) |
| PDF quality level | `saveto/PDFQuality` (100) | **PORT** |
| Replace all unicode characters with &lt;char&gt; | `saveto/ChangeUnicodeCharacter` (false), `ChangeUnicodeCharacterStr` ("_") | **PORT** |
| Auto-generate a folder from the manga's name + manga folder name pattern | `saveto/GenerateMangaFolder` (true), `MangaCustomRename` (`%MANGA%`) | **PORT** |
| Remove manga name from chapter | `saveto/RemoveMangaNameFromChapter` (false) | **PORT** |
| Auto-generate chapter folder + chapter name pattern | `saveto/GenerateChapterFolder` (true), `ChapterCustomRename` (`%CHAPTER%`) | **PORT** |
| Rename digits: Volume (on, length 2), Chapter (on, length 3) | `saveto/ConvertDigitVolume`, `DigitVolumeLength`, `ConvertDigitChapter`, `DigitChapterLength` | **PORT** |
| Filename pattern | `saveto/FilenameCustomRename` (`%FILENAME%`) | **PORT** |
| Rename placeholders: `%NUMBERING%` `%CHAPTER%` `%WEBSITE%` `%MANGA%` `%AUTHOR%` `%ARTIST%` `%FILENAME%` | `baseunits/uBaseUnit.pas:251-257` | **PORT** |
| Image conversion: Save WebP as WebP/PNG/JPEG; PNG compression None/Fastest/Default/Maximum; JPEG quality (80); Save PNG as JPEG | `saveto/ConvertWebP` (1), `PNGCompressionLevel` (1), `JPEGQuality` (80), `PNGSaveAsJPEG` (false) | **PORT** |
| ImageMagick: enable (only if ImageMagick is found), Save image as, Compression type, Quality (75). The format and compression lists are filled from the installed ImageMagick at runtime. | `imagemagick/ImageMagickEnabled,ImageMagickSaveAs,ImageMagickCompression,ImageMagickQuality`; `baseunits/imagemagickmanager.pas` | **PORT** (bundle ImageMagick in the image; final call in #9 and the image-pipeline ticket) |

### 5.5 Updates (`tsUpdate`)

| Option | Key (default) | Tag |
|---|---|---|
| Auto check for latest version. **Also gates the automatic module-update check** (see §7). | `update/AutoCheckLatestVersion` (true) | **SERVER: "new FMD2r release available" banner** (checks releases; no self-install). Split the module-update check into its own setting. |
| Auto check for new chapter at startup | `update/AutoCheckFavStartup` (true) | **PORT** (on server start) |
| Open Favorites at startup (only when the startup check is on) | `update/AutoOpenFavStartup` (false) | **SERVER: default landing page preference** (client-side) |
| Auto check for new chapter in an interval + every N minutes | `update/AutoCheckFavInterval` (true), `AutoCheckFavIntervalMinutes` (60) | **PORT** (server scheduler) |
| Automatic download after checking finishes | `update/AutoCheckFavAutoDownload` (false) | **PORT** |
| Automatically remove completed manga from Favorites | `update/AutoCheckFavAutoRemoveCompletedManga` (false) | **PORT**, but its confirmation dialog is modal in FMD2 (see §7). In a headless server it becomes a pending action plus a notification, or an auto-apply. |
| Don't load manga information when updating the list (filters won't work) | `update/UpdateListNoMangaInfo` (false) | **PORT** |
| Remove duplicate local data when updating the manga list | `update/UpdateListRemoveDuplicateLocalData` (false) | **DROP** (dead option: saved and loaded but never read by the update thread) |

### 5.6 Dialogs (`tsDialogs`): "Show dialog confirmation for"

| Option | Key (default) | Tag |
|---|---|---|
| Exit FMD | `dialogs/ShowQuitDialog` (true) | **DROP** (no exit in a web UI) |
| Delete download/manga/favorite | `dialogs/ShowDeleteDldTaskDialog` (true) | **PORT** |
| Download manga list if empty | `dialogs/ShowDownloadMangalistDialog` (true) | **PORT** (prompt to fetch the FMD2-DB list when a selected site's list is empty) |

### 5.7 Websites (`tsWebsites`): six sub-tabs

| Sub-tab / feature | Source | Tag |
|---|---|---|
| **Websites**: tree of all modules grouped by category, with checkboxes to choose which appear in the Manga Info website selector; Expand/Collapse/Select/Unselect all; search. Initial selection from `config.json` `default_selected_websites`. | `frmMain.lfm:4806-4961`; `frmMain.pas:6390+` | **PORT** |
| **Accounts**: list of modules that support login, with Username, Password, Status (Unknown/Checking/OK/Invalid); Edit (account dialog with show-password); Refresh; delete with confirmation. Login runs in Lua. Stored in `modules.json` → `Account` (Username, Password, Cookies encrypted). | `frmAccountManager.*`, `frmAccountSet.*`; `baseunits/WebsiteModules.pas:78-97,603-614` | **PORT** |
| **Options**: per-module options declared by modules (checkbox, edit, spin, combo). Stored in `modules.json` → `Options`. | `frmWebsiteOptionCustom.*`; `WebsiteModules.pas:68,158-165,591-601` | **PORT** (Host API surface; #3) |
| **Advanced**: per-module settings in a property grid with search: `Enabled` (override on), `MaxTaskLimit`, `MaxThreadPerTaskLimit`, `MaxConnectionLimit`, `UpdateListNumberOfThread`, `UpdateListDirectoryPageNumber`, `HTTP.Cookies`, `HTTP.UserAgent`, `HTTP.Proxy.{ProxyType Default/Direct/HTTP/SOCKS4/SOCKS5, Host, Port, Username, Password}`, `OverrideSettings.SaveToPath`. Stored in `modules.json` → `Settings`. | `frmWebsiteSettings.*`; `baseunits/WebsiteModulesSettings.pas:10-90` | **PORT** |
| **Modules**: Lua module updater. Lists the GitHub repo's `lua/` tree with status NEW / UPDATE / REDOWNLOAD / FAILED / DELETE; Check update; "Show update warning" (prompt before overwriting local changes); "Auto restart" (restart after an update instead of asking); "Enable Module Debug" (shows Check Modules). Shows GitHub API rate stats. | `frmLuaModulesUpdater.*` (strings 155-175); keys `modulesupdater/ShowUpdateWarning` (true), `modulesupdater/AutoRestart` (false), `Modules/Debug` (false); `config.json` `GitHub` block | **SERVER: module updater page with hot-reload instead of an app restart.** "Show update warning" becomes a confirm-before-apply vs. auto-apply setting; "Auto restart" is dropped. Mechanics belong to the module-updater question in #1. |
| **Check Modules** (only when Module Debug is on): runs each module's `OnCheckSite` hook, which fills a `MANGACHECK` object with a sample manga URL, title and chapter URL. It then tests GetInfo and GetPageNumber, per module, with select all/inverse/none, stop, refresh and a log memo. | `frmCheckModules.pas:171-300,970-1250` | **PORT** (debug-gated). It also serves as a ready-made compatibility harness (see Surprises). |

### 5.8 Misc (`tsMisc`)

| Option | Key | Tag |
|---|---|---|
| **Custom color**: colour pickers for Basic list, Manga list, Favorite list, Chapter list and Module list states, light and dark variants, plus Reset | sections `BasicListColors`, `MangaListColors`, `FavoriteListColors`, `ChapterListColor`, `ModuleListColor` (`frmCustomColor.pas:352-421`) | **DROP** (the web theme provides state colours) |
| **Log**: Enable logging, Log file name, Clear log file, Open log (opens the log viewer window: tree log with a limit, stay-on-top and copy) | `logger/Enabled` (false), `logger/LogFileName` (`<exe>.log`); `frmLogger.*` | **SERVER: log viewer page + stdout logs**; a log file stays optional. Details belong to the logging question in #1. |

## 5a. About tab (`tsAbout`)

Source: `frmMain.lfm:5257-5506`; `frmMain.pas:1414-1418,2472-2545`.

| Feature | Tag |
|---|---|
| Version and revision (SHA), loaded-module count | **PORT** |
| About text (`readme.rtf`) and Changelog (`changelog.txt`) sub-tabs | **SERVER: about page with links to release notes** |
| Check for latest version (with abort) | **SERVER: release check** |
| Visit my blog; Donate | **PORT** (as plain links) |

## 6. Dialogs and secondary windows

| Dialog | Source | Tag |
|---|---|---|
| New Chapter Notification: list of "Title &lt;site&gt; has N new chapter(s)" with **Download** / **Add to queue** (added stopped) / **Cancel**. Reused for missing chapters, for "N manga will be removed" (completed) and for "unimported manga". | `frmNewChapter.*`; `uFavoritesManager.pas:954-1130` | **SERVER: non-modal notification/inbox with the same three actions**, plus a notification event |
| Import list: Software = Free Manga Downloader (reads another FMD/FMD2 install's favorites, from a path) or Domdomsoft Manga Downloader; then a list of unimported manga | `frmImportFavorites.*` (`FMDHandle`, `DMDHandle`) | **SERVER: covered by the v1 FMD2 importer** (path or upload). Domdomsoft import: **DROP** (legacy, outside the map's FMD2 importer scope). |
| Transfer Favorites: move favorites to another module; filter All / Valid / Invalid; "Clear downloaded chapter list and reload from server" | `frmTransferFavorites.*` | **PORT** |
| Select directory (Change "Save to") | `frmSelectDirectory.*` | **PORT** (server-side path picker inside the download volume) |
| Account (username/password) | `frmAccountSet.*` | **PORT** |
| Yes/No and custom message dialogs | `frmDialogYesNo.*`, `frmcustommessagedlg.*` | **PORT** (as web confirm dialogs) |
| Shutdown counter (countdown before Exit/Shutdown/Hibernate, with Abort/Now) | `frmShutdownCounter.*`; `frmMain.pas:1884-1912,1967-1993` | **DROP** (power actions) |
| New Version Found (update now / later; FMD closes to update) | `frmUpdateDialog.*`; `baseunits/CheckUpdate.pas`, `SelfUpdater.pas` | **SERVER: release-available banner with a link to release notes** (no self-update) |
| Drop Box window | `frmDropTarget.*` | **SERVER: Add-by-URL box** (see §5.2) |
| Log window | `frmLogger.*` | **SERVER: log viewer page** |
| Website selection ("Select a website") | `frmWebsiteSelection.*` | **DROP** (dead form: declared, but never created anywhere) |

## 7. Background behaviour

| Behaviour | Source | Tag |
|---|---|---|
| **Startup sequence.** Load Lua modules → `modules.json` (settings, options, accounts, cookies) → restore downloads and favorites DBs → load and apply options → app-update and module-update check (if `AutoCheckLatestVersion`) → favorites check (if `AutoCheckFavStartup`) → resume tasks → daily backup → open the selected website's list | `frmMain.pas:2042-2094` | **PORT** (as server boot) |
| **Resume at startup.** Tasks that were Downloading/Preparing/Waiting restart up to `NumberOfTasks` and the rest go to Waiting. Tasks whose module is missing are set to Stopped. | `uDownloadsManager.pas:1859-1893` | **PORT** |
| **Download queue scheduler.** Waiting tasks start while the running count is below `NumberOfTasks` and the module's `CanCreateTask` (per-module `MaxTaskLimit`) allows it. Threads per task are capped by `NumberOfThreadsPerTask` / `MaxThreadPerTaskLimit`; connections per host by `MaxConnectionLimit`. | `uDownloadsManager.pas:1784-1833`; `WebsiteModulesSettings.pas` | **PORT** (#11) |
| **Task retry.** A failed task auto-retries up to `NumberOfAutoRetryFailedTask`; with `AlwaysStartFromFailedChapters` a resumed task restarts from its failed chapters | `uDownloadsManager.pas:1126,1327` | **PORT** |
| **Per-task pipeline**: page list → download images → optional WebP/PNG/JPEG conversion → optional ImageMagick → pack as folder/ZIP/CBZ/PDF/EPUB → mark chapters downloaded | `uDownloadsManager.pas` (statuses `Preparing`, `Downloading`, `Converting...`, `Compressing...`, 281-289) | **PORT** (details in the image-pipeline ticket) |
| **Task finished/failed toast** (balloon hint) | `uDownloadsManager.pas:741-800` | **SERVER: UI toast + notification event** |
| **All downloads finished → "After download finish" action** (Exit/Shutdown/Hibernate) with a countdown | `uDownloadsManager.pas:1824-1826`; `frmMain.pas:1967-1993` | **DROP** the action; **SERVER: "queue drained" notification event** |
| **Favorites check, startup and interval.** `tmCheckFavorites` fires every `AutoCheckFavIntervalMinutes`. The timer is disabled while a check runs and re-enabled when it finishes, so the interval runs from the end of one check to the start of the next. Only enabled favorites with a module and a link are checked. Parallelism is `MaxFavoriteThreads`. A check updates `DateLastChecked`, and `DateLastUpdated` when new chapters are found. New chapters are the chapter links not in the favorite's downloaded-chapter list. | `frmMain.pas:1871-1882`; `uFavoritesManager.pas:360-390,585-600,832-882` | **PORT** |
| **Each interval tick also runs the app-version check and the module-update check** when `AutoCheckLatestVersion` is on | `frmMain.pas:1875-1879` | **SERVER**: release check + module-update check as separate scheduled jobs |
| **Auto-download after check.** With `AutoCheckFavAutoDownload`, new chapters are queued straight away (one task per favorite, chapter names from the rename pattern). Otherwise the New Chapter dialog asks. | `uFavoritesManager.pas:1048-1130` | **PORT** (the dialog becomes a notification/inbox; see §6) |
| **Auto-remove completed manga.** With the option on, favorites with no new chapters and status Completed are listed in a modal "will be removed" dialog and removed on confirm | `uFavoritesManager.pas:1000-1045` | **PORT**, with headless semantics to decide: auto-apply, or a pending confirmation in the UI |
| **Manga list update.** For a site: get the directory → find new titles → get info (unless `UpdateListNoMangaInfo`) → dedupe → save. "%s has %d new manga(s)". Or download the prebuilt list from FMD2-DB (`config.json` `db_url`, a `.7z` per site, extracted with `7za`). | `uUpdateThread.pas:121-136,711-724`; `DBUpdater.pas:41-61` | **PORT** |
| **Background "silent" jobs**: Download all / Add to favorites from the manga list, the drop box or drag-drop; concurrency `MaxBackgroundLoadThreads` | `uSilentThread.pas`; `frmMain.pas:5412-5484` | **PORT** |
| **Module update check** (GitHub API, owner/name/ref/path from `config.json`), then download changed modules, then restart (or prompt) | `frmLuaModulesUpdater.pas` | **SERVER: scheduled check + hot reload** |
| **App update**: check `update_url` (`latest_version.json`), download `updatepackage.7z`, run `updater.exe`, exit | `CheckUpdate.pas:111`; `SelfUpdater.pas`; `FMDOptions.pas` `DO_UPDATE` | **SERVER: release notice only**; updating means pulling a new image or release |
| **Periodic DB flush.** Downloads and favorites are written to their DBs every 10 minutes (`--backup-interval`) and on exit | `frmMain.pas:911,1398-1412,2937-2942`; `md.lpr:129-137` | **SERVER**: the storage design (#15) decides durability; no user setting |
| **Daily settings backup.** At startup (and meant to run hourly), `backup/fmdbackup_yyyymmdd.7z` archives settings.json, accounts.db, modules.json, lua.json, lua_repo.json, downloads.db, downloadedchapters.db and favorites.db. Keeps 7. | `uBackupSettings.pas`; `frmMain.pas:2085,2932-2935` | **SERVER: scheduled snapshot of FMD2r's own data with retention** (or leave it to volume backups; #15) |
| Delete completed tasks on close; vacuum DBs on exit | `frmMain.pas:1444-1447,6249-6250` | **SERVER** (see §5.1) |
| Single-instance IPC ("already running", bring to front) | `md.lpr:162-180`; `frmMain.pas:2912-2921,6224-6243` | **SERVER: data-dir lock** |
| Prevent Windows sleep while downloading (`SetThreadExecutionState`); Windows session-end handling | `frmMain.pas:2015-2025,3320-3336` | **DROP** |
| Restart FMD (after a module update or language change) | `frmMain.pas:2553-2600` | **DROP** (hot reload instead) |

## 8. Non-UI configuration

### `config.json` (shipped next to the exe; `FMDOptions.pas:233-247`, `dist/config.json`)

| Key | Tag |
|---|---|
| `config.default_selected_websites` | **PORT** (first-run default for the website selection) |
| `config.db_url` (`https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/<website>.7z`) | **PORT** (server config) |
| `config.update_url`, `config.update_package_name` | **SERVER: replaced by a release-check URL** |
| `GitHub.{api_url, download_url, owner, name, ref, path}` (module repo `dazedcat19/FMD2` `master` `lua`) | **PORT** (server config for the module updater) |

### Command-line parameters (`mangadownloader/md.lpr:52-140`, `frmMain.pas:2051`)

| Param | Tag |
|---|---|
| `--lua-dofile`: always load Lua modules from file (dev) | **SERVER: dev flag / env var** |
| `--dump-loaded-modules`: log loaded module IDs and names | **PORT** (debug flag) |
| `--dorestart-pid`: Windows restart handshake | **DROP** |
| `--no-commit-queue`, `--max-commit-queue`, `--max-flush-queue`, `--max-big-flush-queue`: SQLite batching tunables | **DROP** (storage-specific; #15 decides its own) |
| `--backup-interval=N` (minutes; DB flush interval, default 10) | **DROP** (see the periodic DB flush) |

### Data files in `userdata/` (`FMDOptions.pas:271-284`), as input for #15 and the importer

`settings.json` (all keys above), `modules.json` (per module: `ID`, `Settings`, `Options`, `Account`, `Cookies`), `favorites.db`, `downloads.db`, `downloadedchapters.db`, `lua.json` / `lua_repo.json` (module-updater state), and `accounts.db` (legacy; see Surprises). Per-site manga lists are `data/<module>.db` (FMD2-DB schema).

## Surprises and upstream bugs

These matter for other tickets.

1. **Settings key typo means one option never persists.** `SaveOptions` writes `connections/AlwaysRetruFailedChaptersOnStart` (`frmMain.pas:6051`), but `LoadOptions` reads `connections/AlwaysStartFromFailedChapters` (`frmMain.pas:5858`). The option always reloads as its default (true). The **importer** should read both keys, and FMD2r should not copy the bug.
2. **The backup timer is miswired.** `TimerBackup` is created, but it is configured through `with Timer1Hour do` (`frmMain.pas:1406-1412`). As a result, `Timer1Hour` runs the 10-minute DB flush instead of the hourly daily-backup check, and `TimerBackup` never fires. In practice the daily 7z backup runs only at startup. Parity should match the intent, not the bug.
3. **Dead options and forms.** `UpdateListRemoveDuplicateLocalData` is saved but never consumed. `OptionEnableCloudflareBypass`, `OptionAutomaticallyDisableCloudflareBypass` and `OptionHTTPUseGzip` are declared in `FMDOptions.pas` with no UI and no consumer. `frmWebsiteSelection` is never instantiated. None of these needs porting. The Cloudflare flags are relevant to #6/#12: Cloudflare handling in FMD2 now lives entirely in Lua (`lua/websitebypass`), not in options.
4. **Accounts and cookies live in `modules.json`, not `accounts.db`.** `ACCOUNTS_FILE` (`accounts.db`) is only referenced by the backup routine. Account username, password and cookies are stored `EncryptString`-encrypted inside `modules.json`, and so are the per-module persisted HTTP cookies (`WebsiteModules.pas:603-625`). This affects the **importer** ticket (credential decryption) and #15.
5. **Upstream has a built-in module self-test.** The debug-only "Check Modules" tab drives each module's `OnCheckSite` Lua hook (a `MANGACHECK` object with a sample manga/chapter URL) and then exercises GetInfo/GetPageNumber. That is a ready-made, upstream-maintained fixture set for **#10 (module compatibility testing)**, and it is a Host API surface that **#3** must include (`MANGACHECK`, `OnCheckSite`, `LuaPushNetStatus`).
6. **7z is a hard dependency of core features.** FMD2-DB lists are `.7z` archives extracted by shelling out to `7za`. App backups and self-update use it too. Keeping FMD2-DB, as the map decides, means FMD2r needs 7z extraction: a vendored C lib or a Rust crate. This is input for **#9 (native dependencies)**.
7. **"Auto check for latest version" also gates module updates.** The same flag triggers the module-update check at startup and on every favorites-interval tick (`frmMain.pas:1875-1879,2073-2077`). FMD2r should give module updates their own schedule (module-updater question in #1).
8. **Several "background" flows rely on modal dialogs.** These are the new-chapter prompt, the completed-manga removal confirmation, the "download list if empty" prompt and the module-update warning. A headless server needs non-blocking equivalents (a pending-actions inbox and notifications). This shapes the **UI prototype (#13)** and the **notifications** question.
9. **Concurrency ceilings differ by OS.** The Linux build caps tasks at 8 and connections per host at 32; Win64 caps them at 64 and 256 (`FMDOptions.pas:36-48`). FMD2r has to pick its own limits (#11), and the importer should clamp imported values.

## Parity checklist (condensed)

This is for #13 and #15.

- **Downloads:** task list with 8 columns; status and history filters, including a custom date range; search; transfer-rate graph; resume/stop all; reorder; stop/resume/redownload/enable/disable; delete task, task + data, or task + data + favorite; delete completed; merge completed; view info; file download in place of open folder.
- **Browse (Manga Info):** website selector; update list, or fetch from FMD2-DB (one site or all); list search and highlight new; advanced filter (fields, 37 genres tri-state, custom genres, any/all, only new, all sites, regex); URL paste-and-go; info panel with cover; chapter list with check helpers, highlight/hide downloaded, filter and sort; download, split download, add as stopped; read online; add to favorites; per-download save-to.
- **Favorites:** list with 9 columns and state badges; all/enabled/disabled; search; check new and check missing, with cancel; enable/disable; view info; download all; rename; transfer website; delete; change save-to with a file move; default action; import from FMD2.
- **Settings:** theme; new-manga days; live search; sort on add; auto-clear completed; connections (timeout, retries, user-agent, proxy, task/thread/favorite/update/background limits, task retry, start from failed); save-to (path, format, PDF quality, renaming patterns and digits, unicode replacement, image conversion, ImageMagick); favorites schedule (startup, interval, auto-download, auto-remove completed); update-list info toggle; confirm-delete and empty-list prompts; show-tab-on-add; load covers.
- **Websites:** selection; accounts; module options; per-module advanced overrides; module updater; Check Modules (debug).
- **System:** notifications (task done/failed, new chapters, queue drained, release available); log viewer; about (version, revision, changelog/release notes); background scheduler; data-dir lock; data snapshots.
