//! The settings as the API shows them: every secret (a password or token) is replaced by a
//! `has_<name>` flag, as accounts show `has_password`, so no response ever carries one.
//!
//! Each view destructures its model type without `..`, so a field added to the model must be
//! added here too.

use serde::Serialize;
use utoipa::ToSchema;

use super::model::{
    ConnectionSettings, CoverSettings, FavoriteSettings, GeneralSettings, ImageSettings,
    LogSettings, ModuleUpdaterSettings, OutputSettings, ProxySettings, ProxyType, SaveToSettings,
    ServerSettings, Settings, UpdateListSettings, XPathSettings,
};
use super::module_overrides::{HttpOverrides, ProxyOverride, ProxyOverrideType};

/// Every application setting, with the secrets left out: [`Settings`] as the API shows it.
/// Each group's schema carries its defaults.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(default)]
pub struct SettingsView {
    pub general: GeneralSettings,
    pub connections: ConnectionSettingsView,
    pub saveto: SaveToSettings,
    pub output: OutputSettings,
    pub images: ImageSettings,
    pub favorites: FavoriteSettings,
    pub update_lists: UpdateListSettings,
    pub module_updater: ModuleUpdaterSettingsView,
    pub server: ServerSettingsView,
    pub xpath: XPathSettings,
    pub covers: CoverSettings,
    pub logs: LogSettings,
}

impl Default for SettingsView {
    fn default() -> Self {
        (&Settings::default()).into()
    }
}

impl From<&Settings> for SettingsView {
    fn from(s: &Settings) -> Self {
        let Settings {
            general,
            connections,
            saveto,
            output,
            images,
            favorites,
            update_lists,
            module_updater,
            server,
            xpath,
            covers,
            logs,
        } = s.clone();
        Self {
            general,
            connections: connections.into(),
            saveto,
            output,
            images,
            favorites,
            update_lists,
            module_updater: module_updater.into(),
            server: server.into(),
            xpath,
            covers,
            logs,
        }
    }
}

/// [`ConnectionSettings`] with the proxy password left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ConnectionSettingsView {
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
    /// Default user agent; an empty value resets to
    /// [`DEFAULT_USER_AGENT`](super::DEFAULT_USER_AGENT) (`connections/DefaultUserAgent`,
    /// mangadownloader/forms/frmMain.pas:5867-5871).
    pub user_agent: String,
    /// Global proxy (mangadownloader/forms/frmMain.pas:5874-5879).
    pub proxy: ProxySettingsView,
    /// FlareSolverr's URL, e.g. `http://flaresolverr:8191`, for Cloudflare challenges; empty
    /// for none. No FMD2 setting: FMD2r writes it into upstream's
    /// `lua/websitebypass/websitebypass_config.json` at startup
    /// ([`write_websitebypass_config`](super::write_websitebypass_config)).
    pub flaresolverr_url: String,
}

impl From<ConnectionSettings> for ConnectionSettingsView {
    fn from(c: ConnectionSettings) -> Self {
        let ConnectionSettings {
            max_parallel_tasks,
            threads_per_task,
            retry_count,
            auto_retry_failed_tasks,
            always_start_from_failed_chapters,
            max_favorite_threads,
            max_update_list_threads,
            timeout_secs,
            user_agent,
            proxy,
            flaresolverr_url,
        } = c;
        Self {
            max_parallel_tasks,
            threads_per_task,
            retry_count,
            auto_retry_failed_tasks,
            always_start_from_failed_chapters,
            max_favorite_threads,
            max_update_list_threads,
            timeout_secs,
            user_agent,
            proxy: proxy.into(),
            flaresolverr_url,
        }
    }
}

/// [`ProxySettings`] with the password left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ProxySettingsView {
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
    /// Whether a password is set. Patch `password` to set it; an empty one clears it.
    pub has_password: bool,
}

impl From<ProxySettings> for ProxySettingsView {
    fn from(p: ProxySettings) -> Self {
        let ProxySettings {
            enabled,
            kind,
            host,
            port,
            username,
            password,
        } = p;
        Self {
            enabled,
            kind,
            host,
            port,
            username,
            has_password: !password.is_empty(),
        }
    }
}

/// [`ModuleUpdaterSettings`] with the GitHub token left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModuleUpdaterSettingsView {
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
    /// Whether a GitHub token is set. Patch `github_token` to set it; an empty one clears it.
    pub has_github_token: bool,
    /// Keep the previous version of a module that fails to load after an update. No FMD2
    /// counterpart.
    pub keep_last_good: bool,
}

impl From<ModuleUpdaterSettings> for ModuleUpdaterSettingsView {
    fn from(m: ModuleUpdaterSettings) -> Self {
        let ModuleUpdaterSettings {
            auto_update,
            interval_minutes,
            repo_owner,
            repo_name,
            repo_ref,
            repo_path,
            github_token,
            keep_last_good,
        } = m;
        Self {
            auto_update,
            interval_minutes,
            repo_owner,
            repo_name,
            repo_ref,
            repo_path,
            has_github_token: is_set(github_token.as_deref()),
            keep_last_good,
        }
    }
}

/// [`ServerSettings`] with the password left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ServerSettingsView {
    /// Socket address to listen on.
    pub bind: String,
    /// Whether a password is set. Patch `auth_token` to set it; an empty one clears it.
    pub has_auth_token: bool,
    /// Days a login session may go unused before it ends; every authorized request restarts
    /// the count.
    #[schema(minimum = 1, maximum = 365)]
    pub session_idle_days: u32,
    /// Days a login session lasts at most, however often it is used.
    #[schema(minimum = 1, maximum = 3650)]
    pub session_lifetime_days: u32,
}

impl From<ServerSettings> for ServerSettingsView {
    fn from(s: ServerSettings) -> Self {
        let ServerSettings {
            bind,
            auth_token,
            session_idle_days,
            session_lifetime_days,
        } = s;
        Self {
            bind,
            has_auth_token: is_set(auth_token.as_deref()),
            session_idle_days,
            session_lifetime_days,
        }
    }
}

/// [`HttpOverrides`] with the proxy password left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HttpOverridesView {
    /// Replaces the default user agent when non-empty.
    pub user_agent: String,
    /// Cookies merged into every request (`HTTP.Cookies`,
    /// baseunits/WebsiteModulesSettings.pas:41).
    pub cookies: String,
    pub proxy: ProxyOverrideView,
}

impl From<HttpOverrides> for HttpOverridesView {
    fn from(h: HttpOverrides) -> Self {
        let HttpOverrides {
            user_agent,
            cookies,
            proxy,
        } = h;
        Self {
            user_agent,
            cookies,
            proxy: proxy.into(),
        }
    }
}

/// [`ProxyOverride`] with the password left out.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ProxyOverrideView {
    #[serde(rename = "type")]
    pub kind: ProxyOverrideType,
    pub host: String,
    pub port: String,
    pub username: String,
    /// Whether a password is set. Patch `password` to set it; an empty one clears it.
    pub has_password: bool,
}

impl From<ProxyOverride> for ProxyOverrideView {
    fn from(p: ProxyOverride) -> Self {
        let ProxyOverride {
            kind,
            host,
            port,
            username,
            password,
        } = p;
        Self {
            kind,
            host,
            port,
            username,
            has_password: !password.is_empty(),
        }
    }
}

fn is_set(secret: Option<&str>) -> bool {
    secret.is_some_and(|s| !s.is_empty())
}
