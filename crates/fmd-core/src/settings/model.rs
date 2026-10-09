//! The typed settings tree and its FMD2 defaults.
//!
//! Groups follow FMD2's options dialog and `settings.json` sections. Defaults come from
//! `TMainForm.LoadOptions` (mangadownloader/forms/frmMain.pas:5803-5980) where it passes a literal
//! default, otherwise from the `Option*` variables it falls back to (baseunits/FMDOptions.pas).
//! Where the two disagree (`GenerateMangaFolder`, `PDFQuality`), `LoadOptions` wins because that
//! is what a fresh FMD2 install shows. Settings with no FMD2 counterpart say so.
//!
//! Every group is `#[serde(default)]`, so a stored group missing a field (written by an older
//! build) gets that field's default.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Every application setting. Stored one group per key in `app.db`'s `settings` table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub connections: ConnectionSettings,
    pub saveto: SaveToSettings,
    pub output: OutputSettings,
    pub images: ImageSettings,
    pub favorites: FavoriteSettings,
    pub update_lists: UpdateListSettings,
    pub module_updater: ModuleUpdaterSettings,
    pub server: ServerSettings,
    pub xpath: XPathSettings,
    pub covers: CoverSettings,
    pub logs: LogSettings,
    pub metadata: MetadataSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct GeneralSettings {
    /// Folder for the list databases, relative to the app data directory (`DATA_FOLDER`,
    /// baseunits/FMDOptions.pas:285).
    pub data_dir: String,
    /// Folder holding the Lua tree (`LUA_REPO_FOLDER`, baseunits/FMDOptions.pas:297).
    pub lua_dir: String,
    /// UI language code (`languages/Selected`, default `en`,
    /// mangadownloader/forms/frmMain.pas:6897).
    pub language: String,
    /// Add new tasks stopped instead of waiting (`general/AddAsStopped`,
    /// mangadownloader/forms/frmMain.pas:5819).
    pub add_as_stopped: bool,
    /// Load manga covers (`view/LoadMangaCover`, mangadownloader/forms/frmMain.pas:5831).
    pub load_covers: bool,
    /// The websites Discover lists and searches, by module ID (`general/MangaListSelect`,
    /// comma-separated, mangadownloader/forms/frmMain.pas:5990-6004). IDs of modules that are
    /// not loaded are kept but ignored, so a module that comes back is still selected; FMD2
    /// drops them on load (mangadownloader/forms/frmMain.pas:6464-6470). Empty on a fresh
    /// install: FMD2 defaults to `config.json`'s `default_selected_websites`
    /// (baseunits/FMDOptions.pas:94, :250), which FMD2r does not ship.
    pub selected_websites: Vec<String>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            data_dir: "data".into(),
            lua_dir: "lua".into(),
            language: "en".into(),
            add_as_stopped: false,
            load_covers: true,
            selected_websites: Vec::new(),
        }
    }
}

/// User agent FMD2 sends when none is configured (`UserAgentDefault`,
/// baseunits/httpsendthread.pas:160).
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ConnectionSettings {
    /// Tasks downloading at once, over all modules (`connections/NumberOfTasks`,
    /// `OptionMaxParallel` = 1, baseunits/FMDOptions.pas:133). Range 1..=64: the Win64
    /// `MAX_TASKLIMIT` (baseunits/FMDOptions.pas:42), so any FMD2 value imports.
    #[schema(minimum = 1, maximum = 64)]
    pub max_parallel_tasks: u32,
    /// Page download threads per task (`connections/NumberOfThreadsPerTask`, `OptionMaxThreads`
    /// = 1, baseunits/FMDOptions.pas:134). Range 1..=256: the Win64 `MAX_CONNECTIONPERHOSTLIMIT`
    /// (baseunits/FMDOptions.pas:43).
    #[schema(minimum = 1, maximum = 256)]
    pub threads_per_task: u32,
    /// HTTP retries per request; -1 retries forever (`connections/Retry`, `OptionMaxRetry` = 5,
    /// baseunits/FMDOptions.pas:135; -1..=5 in mangadownloader/forms/frmMain.lfm:3633-3634;
    /// meaning in baseunits/httpsendthread.pas:626).
    #[schema(minimum = -1, maximum = 5)]
    pub retry_count: i32,
    /// Times a failed task is restarted automatically (`connections/NumberOfAutoRetryFailedTask`,
    /// `OptionRetryFailedTask` = 1, baseunits/FMDOptions.pas:136). Range 0..=100, the TSpinEdit default (no bounds
    /// set, mangadownloader/forms/frmMain.lfm:3669-3681).
    #[schema(minimum = 0, maximum = 100)]
    pub auto_retry_failed_tasks: u32,
    /// Restart a task from its failed chapters (`connections/AlwaysStartFromFailedChapters`,
    /// baseunits/FMDOptions.pas:137).
    pub always_start_from_failed_chapters: bool,
    /// Threads checking favorites (`connections/MaxFavoriteThreads`,
    /// baseunits/FMDOptions.pas:130). Range 1..=32 (mangadownloader/forms/frmMain.lfm:3786-3787).
    #[schema(minimum = 1, maximum = 32)]
    pub max_favorite_threads: u32,
    /// Threads updating manga lists (`connections/MaxUpdateListThreads`,
    /// baseunits/FMDOptions.pas:131). Range 1..=32 (mangadownloader/forms/frmMain.lfm:3827-3828).
    #[schema(minimum = 1, maximum = 32)]
    pub max_update_list_threads: u32,
    /// Connection timeout in seconds (`connections/ConnectionTimeout`, `OptionConnectionTimeout`
    /// = 30, baseunits/FMDOptions.pas:129). Range 1..=300
    /// (mangadownloader/forms/frmMain.lfm:3387-3388).
    #[schema(minimum = 1, maximum = 300)]
    pub timeout_secs: u32,
    /// Default user agent; an empty value resets to [`DEFAULT_USER_AGENT`]
    /// (`connections/DefaultUserAgent`, mangadownloader/forms/frmMain.pas:5867-5871).
    pub user_agent: String,
    /// Global proxy (mangadownloader/forms/frmMain.pas:5874-5879).
    pub proxy: ProxySettings,
    /// FlareSolverr's URL, e.g. `http://flaresolverr:8191`, for Cloudflare challenges; empty
    /// for none. No FMD2 setting: FMD2r writes it into upstream's
    /// `lua/websitebypass/websitebypass_config.json` at startup
    /// ([`write_websitebypass_config`](super::write_websitebypass_config)).
    pub flaresolverr_url: String,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            max_parallel_tasks: 1,
            threads_per_task: 1,
            retry_count: 5,
            auto_retry_failed_tasks: 1,
            always_start_from_failed_chapters: true,
            max_favorite_threads: 1,
            max_update_list_threads: 1,
            timeout_secs: 30,
            user_agent: DEFAULT_USER_AGENT.into(),
            proxy: ProxySettings::default(),
            flaresolverr_url: String::new(),
        }
    }
}

/// Global proxy. The password is stored encrypted in `app.db` and the API never returns it
/// (see `secrets.rs` and [`ProxySettingsView`](super::ProxySettingsView)).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ProxySettings {
    /// `connections/UseProxy`, default false.
    pub enabled: bool,
    /// `connections/ProxyType`, default `HTTP`.
    #[serde(rename = "type")]
    pub kind: ProxyType,
    pub host: String,
    /// `connections/Port`, default empty.
    #[schema(minimum = 1, maximum = 65535)]
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
}

/// `cbOptionProxyType` items (mangadownloader/forms/frmMain.lfm:3559-3563).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    #[default]
    Http,
    Socks4,
    Socks5,
}

/// Default download directory (`DEFAULT_PATH`, baseunits/FMDOptions.pas:283).
pub const DEFAULT_PATH: &str = "downloads";

/// The rename templates' defaults (baseunits/FMDOptions.pas:24-26).
pub const DEFAULT_MANGA_CUSTOMRENAME: &str = "%MANGA%";
pub const DEFAULT_CHAPTER_CUSTOMRENAME: &str = "%CHAPTER%";
pub const DEFAULT_FILENAME_CUSTOMRENAME: &str = "%FILENAME%";

/// Where and under which names downloads are saved. The templates take the tokens `%MANGA%`,
/// `%CHAPTER%`, `%NUMBERING%`, `%WEBSITE%`, `%AUTHOR%`, `%ARTIST%` and `%FILENAME%`
/// (baseunits/uBaseUnit.pas:251-257); an empty template resets to its default
/// (mangadownloader/forms/frmMain.pas:5894-5917).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SaveToSettings {
    /// Download directory; empty resets to the default (`saveto/SaveTo`, `DEFAULT_PATH`,
    /// baseunits/FMDOptions.pas:283, mangadownloader/forms/frmMain.pas:5882-5886).
    pub default_dir: String,
    /// `saveto/GenerateMangaFolder`, default true (mangadownloader/forms/frmMain.pas:5893).
    pub generate_manga_folder: bool,
    /// `saveto/MangaCustomRename` (mangadownloader/forms/frmMain.pas:5894).
    pub manga_rename: String,
    /// `saveto/GenerateChapterFolder`, default true (mangadownloader/forms/frmMain.pas:5900).
    pub generate_chapter_folder: bool,
    /// `saveto/ChapterCustomRename` (mangadownloader/forms/frmMain.pas:5901).
    pub chapter_rename: String,
    /// `saveto/FilenameCustomRename` (mangadownloader/forms/frmMain.pas:5913).
    pub filename_rename: String,
    /// `saveto/RemoveMangaNameFromChapter`, default false
    /// (mangadownloader/forms/frmMain.pas:5892).
    pub remove_manga_name_from_chapter: bool,
    /// Replace non-ASCII characters in names (`saveto/ChangeUnicodeCharacter`, default false,
    /// mangadownloader/forms/frmMain.pas:5890).
    pub replace_unicode: bool,
    /// Replacement for non-ASCII characters (`saveto/ChangeUnicodeCharacterStr`,
    /// `OptionChangeUnicodeCharacterStr` = `_`, baseunits/FMDOptions.pas:109).
    pub replace_unicode_with: String,
    /// `saveto/ConvertDigitVolume`, default true (mangadownloader/forms/frmMain.pas:5907).
    pub convert_digit_volume: bool,
    /// `saveto/DigitVolumeLength`, default 2 (mangadownloader/forms/frmMain.pas:5908). Range
    /// 1..=10 (mangadownloader/forms/frmMain.lfm:4111-4112).
    #[schema(minimum = 1, maximum = 10)]
    pub digit_volume_length: u32,
    /// `saveto/ConvertDigitChapter`, default true (mangadownloader/forms/frmMain.pas:5910).
    pub convert_digit_chapter: bool,
    /// `saveto/DigitChapterLength`, default 3 (mangadownloader/forms/frmMain.pas:5911). Range
    /// 1..=10 (mangadownloader/forms/frmMain.lfm:4138-4139).
    #[schema(minimum = 1, maximum = 10)]
    pub digit_chapter_length: u32,
    /// Which characters are stripped from names. FMD2 always strips the Windows set
    /// (`RemoveSymbols`, baseunits/uBaseUnit.pas:1382); FMD2r runs on Linux and defaults to
    /// POSIX.
    pub illegal_chars: SymbolMode,
}

impl Default for SaveToSettings {
    fn default() -> Self {
        Self {
            default_dir: DEFAULT_PATH.into(),
            generate_manga_folder: true,
            manga_rename: DEFAULT_MANGA_CUSTOMRENAME.into(),
            generate_chapter_folder: true,
            chapter_rename: DEFAULT_CHAPTER_CUSTOMRENAME.into(),
            filename_rename: DEFAULT_FILENAME_CUSTOMRENAME.into(),
            remove_manga_name_from_chapter: false,
            replace_unicode: false,
            replace_unicode_with: "_".into(),
            convert_digit_volume: true,
            digit_volume_length: 2,
            convert_digit_chapter: true,
            digit_chapter_length: 3,
            illegal_chars: SymbolMode::Posix,
        }
    }
}

/// How characters that are illegal in file names are handled (mirrors `fmd_pack::SymbolMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SymbolMode {
    /// Replace `/` and NUL with `_`.
    #[default]
    Posix,
    /// Delete `\ / : * ? " < > |`, tab and `;` (baseunits/uBaseUnit.pas:59-60).
    Windows,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct OutputSettings {
    /// `saveto/Compress`, default 0 = None (mangadownloader/forms/frmMain.pas:5889).
    pub format: OutputFormat,
    /// JPEG quality of PDF pages (`saveto/PDFQuality`, default 100,
    /// mangadownloader/forms/frmMain.pas:5888). Range 5..=100
    /// (mangadownloader/forms/frmMain.lfm:4245-4246).
    #[schema(minimum = 5, maximum = 100)]
    pub pdf_quality: u32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            format: OutputFormat::Folder,
            pdf_quality: 100,
        }
    }
}

/// `rgOptionCompress` items None/ZIP/CBZ/PDF/EPUB (mangadownloader/forms/frmMain.lfm:3912-3918).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Keep the images in a folder (FMD2's "None").
    #[default]
    Folder,
    Zip,
    Cbz,
    Pdf,
    Epub,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ImageSettings {
    /// `saveto/PNGSaveAsJPEG`, `OptionPNGSaveAsJPEG` = false (baseunits/FMDOptions.pas:123).
    pub png_to_jpeg: bool,
    /// `saveto/ConvertWebP`, `OptionWebPSaveAs` = 1 = PNG (baseunits/FMDOptions.pas:124).
    pub webp_save_as: WebpSaveAs,
    /// `saveto/PNGCompressionLevel`, `OptionPNGCompressionLevel` = 1 = Fastest
    /// (baseunits/FMDOptions.pas:125).
    pub png_compression: PngCompression,
    /// `saveto/JPEGQuality`, `OptionJPEGQuality` = 80 (baseunits/FMDOptions.pas:126). Range
    /// 1..=100 (mangadownloader/forms/frmMain.lfm:4426-4427).
    #[schema(minimum = 1, maximum = 100)]
    pub jpeg_quality: u32,
    pub imagemagick: ImageMagickSettings,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            png_to_jpeg: false,
            webp_save_as: WebpSaveAs::Png,
            png_compression: PngCompression::Fastest,
            jpeg_quality: 80,
            imagemagick: ImageMagickSettings::default(),
        }
    }
}

/// `cbWebPSaveAs` items (mangadownloader/forms/frmMain.lfm:4363-4367).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum WebpSaveAs {
    /// Keep WebP.
    Webp,
    #[default]
    Png,
    Jpeg,
}

/// `cbPNGCompressionLevel` items (mangadownloader/forms/frmMain.lfm:4395-4400).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PngCompression {
    None,
    #[default]
    Fastest,
    Default,
    Maximum,
}

/// Optional ImageMagick conversion (mangadownloader/forms/frmMain.pas:5925-5942).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ImageMagickSettings {
    /// `imagemagick/ImageMagickEnabled`, default false.
    pub enabled: bool,
    /// Target format (`imagemagick/ImageMagickSaveAs`, default `JPEG`).
    pub save_as: String,
    /// `-compress` type (`imagemagick/ImageMagickCompression`, default `None`).
    pub compression: String,
    /// `imagemagick/ImageMagickQuality`, default 75. Range 1..=100
    /// (mangadownloader/forms/frmMain.lfm:4555-4556).
    #[schema(minimum = 1, maximum = 100)]
    pub quality: u32,
}

impl Default for ImageMagickSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            save_as: "JPEG".into(),
            compression: "None".into(),
            quality: 75,
        }
    }
}

/// New-chapter checks for the library (mangadownloader/forms/frmMain.pas:5946-5955).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct FavoriteSettings {
    /// `update/AutoCheckFavStartup`, default true.
    pub check_at_startup: bool,
    /// `update/AutoCheckFavInterval`, default true.
    pub check_on_interval: bool,
    /// `update/AutoCheckFavIntervalMinutes`, default 60. Range 1..=1440
    /// (mangadownloader/forms/frmMain.lfm:4648-4649).
    #[schema(minimum = 1, maximum = 1440)]
    pub check_interval_minutes: u32,
    /// Queue new chapters automatically (`update/AutoCheckFavAutoDownload`, default false).
    pub auto_download: bool,
    /// `update/AutoCheckFavAutoRemoveCompletedManga`, default false.
    pub remove_completed: bool,
}

impl Default for FavoriteSettings {
    fn default() -> Self {
        Self {
            check_at_startup: true,
            check_on_interval: true,
            check_interval_minutes: 60,
            auto_download: false,
            remove_completed: false,
        }
    }
}

/// FMD2-DB download URL (`db_url`, dist/config.json:4); `<website>` is replaced by the module
/// ID (`GetDBURL(FModule.ID)`, baseunits/DBUpdater.pas:125).
pub const DEFAULT_DB_URL: &str =
    "https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/<website>.7z";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct UpdateListSettings {
    /// Update the selected modules' lists on a timer. No FMD2 counterpart (FMD2 only updates
    /// lists on demand); off by default.
    pub auto_update: bool,
    /// Hours between automatic list updates. No FMD2 counterpart. Minimum 1.
    #[schema(minimum = 1)]
    pub interval_hours: u32,
    /// `update/UpdateListNoMangaInfo`, default false (mangadownloader/forms/frmMain.pas:5956).
    pub no_manga_info: bool,
    /// `update/UpdateListRemoveDuplicateLocalData`, default false
    /// (mangadownloader/forms/frmMain.pas:5957).
    pub remove_duplicate_local_data: bool,
    /// Days a list entry counts as new (`update/NewMangaTime`, default 1,
    /// mangadownloader/forms/frmMain.pas:5953). Range 1..=365
    /// (mangadownloader/forms/frmMain.lfm:2950-2951).
    #[schema(minimum = 1, maximum = 365)]
    pub new_manga_days: u32,
    /// FMD2-DB URL template ([`DEFAULT_DB_URL`]); `<website>` is replaced by the module ID.
    pub db_url: String,
}

impl Default for UpdateListSettings {
    fn default() -> Self {
        Self {
            auto_update: false,
            interval_hours: 24,
            no_manga_info: false,
            remove_duplicate_local_data: false,
            new_manga_days: 1,
            db_url: DEFAULT_DB_URL.into(),
        }
    }
}

/// Lua module sync from GitHub. The repo defaults are FMD2's `GitHub` section
/// (dist/config.json:8-15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ModuleUpdaterSettings {
    /// Sync at startup and on the interval. No FMD2 counterpart: FMD2 checks when the user
    /// clicks "Check update" (`btCheckUpdateClick`, mangadownloader/forms/frmLuaModulesUpdater.pas:70).
    pub auto_update: bool,
    /// Minutes between syncs. No FMD2 counterpart. Minimum 1.
    #[schema(minimum = 1)]
    pub interval_minutes: u32,
    pub repo_owner: String,
    pub repo_name: String,
    pub repo_ref: String,
    pub repo_path: String,
    /// Optional GitHub token to raise the API rate limit. No FMD2 counterpart. Stored encrypted;
    /// the API only shows whether it is set.
    pub github_token: Option<String>,
    /// Keep the previous version of a module that fails to load after an update. No FMD2
    /// counterpart.
    pub keep_last_good: bool,
}

impl Default for ModuleUpdaterSettings {
    fn default() -> Self {
        Self {
            auto_update: true,
            interval_minutes: 60,
            repo_owner: "dazedcat19".into(),
            repo_name: "FMD2".into(),
            repo_ref: "master".into(),
            repo_path: "lua".into(),
            github_token: None,
            keep_last_good: true,
        }
    }
}

/// The HTTP server. No FMD2 counterpart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ServerSettings {
    /// Socket address to listen on, from the next start. `--bind` / `FMD2R_BIND` overrides it.
    pub bind: String,
    /// A salted hash of the password/bearer token clients must present; `None` disables auth.
    /// A patch sets it to a password, which is hashed before it is stored; the API only shows
    /// whether it is set. `--password` / `FMD2R_PASSWORD` overrides it.
    pub auth_token: Option<String>,
    /// Days a login session may go unused before it ends; every authorized request restarts
    /// the count.
    #[schema(minimum = 1, maximum = 365)]
    pub session_idle_days: u32,
    /// Days a login session lasts at most, however often it is used.
    #[schema(minimum = 1, maximum = 3650)]
    pub session_lifetime_days: u32,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
            auth_token: None,
            session_idle_days: 7,
            session_lifetime_days: 30,
        }
    }
}

/// Module XPath evaluation. No FMD2 counterpart: FMD2 always uses its own engine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct XPathSettings {
    /// Which `fmd-xpath` backend evaluates module XPath (`fmd_lua::Runtime::set_xpath_backend`).
    pub backend: XPathBackend,
}

/// The `fmd-xpath` backends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum XPathBackend {
    /// FMD2's own engine (internettools) over FFI, so module XPath behaves exactly as in FMD2.
    /// Needs a build with the `fpc` backend.
    Fpc,
    /// The pure-Rust engine; the default since the differential corpus showed parity with
    /// `fpc` (T35, fixtures/xpath-corpus).
    #[default]
    Native,
}

/// The cover proxy's disk cache (`/api/covers`). No FMD2 counterpart: FMD2 keeps the one cover it
/// shows in memory (baseunits/uGetMangaInfosThread.pas:168-191).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct CoverSettings {
    /// Hours a cached cover is served before the site is asked whether it changed; 0 asks every
    /// time.
    pub revalidate_after_hours: u32,
    /// Size cap of the cover cache in MiB; least recently used covers are evicted past it.
    /// Minimum 1.
    #[schema(minimum = 1)]
    pub cache_size_mb: u32,
}

impl Default for CoverSettings {
    fn default() -> Self {
        Self {
            revalidate_after_hours: 7 * 24,
            cache_size_mb: 256,
        }
    }
}

/// The log files in `<data dir>/logs/`. No FMD2 counterpart: FMD2 appends to one unbounded log
/// file through MultiLog (baseunits/uBaseUnit.pas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct LogSettings {
    /// Size in MiB a log file grows to before the next one is started. 1 to 1024.
    #[schema(minimum = 1, maximum = 1024)]
    pub max_file_size_mb: u32,
    /// Log files kept, the one being written included; the oldest is deleted past it. 1 to 100.
    #[schema(minimum = 1, maximum = 100)]
    pub max_files: u32,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            max_file_size_mb: 10,
            max_files: 5,
        }
    }
}

/// Metadata for list titles from outside the websites (T73). No FMD2 counterpart: FMD2 uses only
/// the websites' own metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct MetadataSettings {
    pub mangabaka: MangaBakaSettings,
}

/// The local copy of MangaBaka's database (`metadata.db`), downloaded only when asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct MangaBakaSettings {
    /// Days between automatic refreshes of a downloaded database; 0 turns them off. 0 to 365.
    #[schema(minimum = 0, maximum = 365)]
    pub refresh_days: u32,
}

impl Default for MangaBakaSettings {
    fn default() -> Self {
        Self { refresh_days: 7 }
    }
}
