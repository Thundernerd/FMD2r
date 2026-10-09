//! Range checks and FMD2's empty-value fallbacks, applied to every update.

use std::net::SocketAddr;
use std::ops::RangeInclusive;

use super::model::{
    DEFAULT_CHAPTER_CUSTOMRENAME, DEFAULT_FILENAME_CUSTOMRENAME, DEFAULT_MANGA_CUSTOMRENAME,
    DEFAULT_PATH, DEFAULT_USER_AGENT, SaveToSettings, Settings,
};
use super::service::FieldError;

/// Resets blank values to their defaults as FMD2 does: the user agent
/// (mangadownloader/forms/frmMain.pas:6279-6285), the download directory and the rename
/// templates (mangadownloader/forms/frmMain.pas:5882-5917). An empty GitHub token or server
/// password becomes `None`.
pub(crate) fn normalize(s: &mut Settings) {
    fn reset_blank(value: &mut String, default: &str) {
        if value.trim().is_empty() {
            *value = default.to_string();
        }
    }
    let c = &mut s.connections;
    c.user_agent = c.user_agent.trim().to_string();
    reset_blank(&mut c.user_agent, DEFAULT_USER_AGENT);
    for secret in [&mut s.module_updater.github_token, &mut s.server.auth_token] {
        if secret.as_deref() == Some("") {
            *secret = None;
        }
    }
    let saveto = &mut s.saveto;
    reset_blank(&mut saveto.default_dir, DEFAULT_PATH);
    reset_blank(&mut saveto.manga_rename, DEFAULT_MANGA_CUSTOMRENAME);
    reset_blank(&mut saveto.chapter_rename, DEFAULT_CHAPTER_CUSTOMRENAME);
    reset_blank(&mut saveto.filename_rename, DEFAULT_FILENAME_CUSTOMRENAME);
    for destination in &mut saveto.destinations {
        destination.name = destination.name.trim().to_string();
        destination.path = destination.path.trim().to_string();
    }
}

/// Keeps `saveto.default_dir` and the default destination in step: a patch that changed only
/// `default_dir` moves the default destination (for clients that only know `default_dir`);
/// otherwise `default_dir` follows the default destination.
pub(crate) fn sync_default_dir(current: &SaveToSettings, next: &mut SaveToSettings) {
    if next.default_dir != current.default_dir && next.destinations == current.destinations {
        let dir = next.default_dir.clone();
        if let Some(default) = next.destinations.iter_mut().find(|d| d.default) {
            default.path = dir;
        }
    }
    next.default_dir = next.default_path().to_string();
}

/// Empty or duplicate (ignoring case) names, empty paths, and not exactly one default.
pub(super) fn validate_destinations(
    current: &SaveToSettings,
    next: &SaveToSettings,
) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (i, destination) in next.destinations.iter().enumerate() {
        let name = destination.name.to_lowercase();
        if name.is_empty() {
            errors.push(FieldError::new(
                format!("saveto.destinations.{i}.name"),
                "a destination needs a name",
            ));
        } else if seen.contains(&name) {
            errors.push(FieldError::new(
                format!("saveto.destinations.{i}.name"),
                format!("another destination is named {}", destination.name),
            ));
        }
        seen.push(name);
        if destination.path.is_empty() {
            errors.push(FieldError::new(
                format!("saveto.destinations.{i}.path"),
                "a destination needs a folder",
            ));
        }
    }
    let defaults = next.destinations.iter().filter(|d| d.default).count();
    if defaults != 1 {
        let removed = current
            .destinations
            .iter()
            .find(|d| d.default)
            .is_some_and(|old| !next.destinations.iter().any(|d| d.name == old.name));
        let reason = if removed && defaults == 0 {
            "the default destination cannot be removed"
        } else {
            "exactly one destination must be the default"
        };
        errors.push(FieldError::new("saveto.destinations", reason));
    }
    errors
}

/// Checks numeric settings against FMD2's spin edit ranges (cited in `model.rs`, mirrored by its
/// `#[schema(minimum, maximum)]`) and FMD2r-only settings against their own constraints.
pub(super) fn validate(s: &Settings) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let mut check = |field: &str, error: Option<String>| {
        errors.extend(error.map(|reason| FieldError::new(field, reason)));
    };
    let c = &s.connections;
    check(
        "connections.max_parallel_tasks",
        out_of(c.max_parallel_tasks, 1..=64),
    );
    check(
        "connections.threads_per_task",
        out_of(c.threads_per_task, 1..=256),
    );
    check("connections.retry_count", out_of(c.retry_count, -1..=5));
    check(
        "connections.auto_retry_failed_tasks",
        out_of(c.auto_retry_failed_tasks, 0..=100),
    );
    check(
        "connections.max_favorite_threads",
        out_of(c.max_favorite_threads, 1..=32),
    );
    check(
        "connections.max_update_list_threads",
        out_of(c.max_update_list_threads, 1..=32),
    );
    check("connections.timeout_secs", out_of(c.timeout_secs, 1..=300));
    check(
        "connections.proxy.port",
        (c.proxy.port == Some(0)).then(|| "must be 1..=65535".into()),
    );
    check(
        "saveto.digit_volume_length",
        out_of(s.saveto.digit_volume_length, 1..=10),
    );
    check(
        "saveto.digit_chapter_length",
        out_of(s.saveto.digit_chapter_length, 1..=10),
    );
    check("output.pdf_quality", out_of(s.output.pdf_quality, 5..=100));
    check(
        "images.jpeg_quality",
        out_of(s.images.jpeg_quality, 1..=100),
    );
    check(
        "images.imagemagick.quality",
        out_of(s.images.imagemagick.quality, 1..=100),
    );
    check(
        "favorites.check_interval_minutes",
        out_of(s.favorites.check_interval_minutes, 1..=1440),
    );
    check(
        "update_lists.interval_hours",
        out_of(s.update_lists.interval_hours, 1..=u32::MAX),
    );
    check(
        "update_lists.new_manga_days",
        out_of(s.update_lists.new_manga_days, 1..=365),
    );
    check(
        "module_updater.interval_minutes",
        out_of(s.module_updater.interval_minutes, 1..=u32::MAX),
    );
    check(
        "server.session_idle_days",
        out_of(s.server.session_idle_days, 1..=365),
    );
    check(
        "server.session_lifetime_days",
        out_of(s.server.session_lifetime_days, 1..=3650),
    );
    check(
        "covers.cache_size_mb",
        out_of(s.covers.cache_size_mb, 1..=u32::MAX),
    );
    check(
        "logs.max_file_size_mb",
        out_of(s.logs.max_file_size_mb, 1..=1024),
    );
    check("logs.max_files", out_of(s.logs.max_files, 1..=100));
    check(
        "metadata.mangabaka.refresh_days",
        out_of(s.metadata.mangabaka.refresh_days, 0..=365),
    );
    let bad_flaresolverr = !c.flaresolverr_url.trim().is_empty()
        && super::websitebypass::flaresolverr_address(&c.flaresolverr_url).is_none();
    check(
        "connections.flaresolverr_url",
        bad_flaresolverr
            .then(|| "must be empty or an http(s) URL such as http://flaresolverr:8191".into()),
    );
    check(
        "server.bind",
        s.server
            .bind
            .parse::<SocketAddr>()
            .is_err()
            .then(|| "must be a socket address such as 0.0.0.0:8080".into()),
    );
    errors
}

/// Why `value` is not in `range`, if it is not.
fn out_of<T>(value: T, range: RangeInclusive<T>) -> Option<String>
where
    T: PartialOrd + std::fmt::Display,
{
    (!range.contains(&value))
        .then(|| format!("{value} is outside {}..={}", range.start(), range.end()))
}
