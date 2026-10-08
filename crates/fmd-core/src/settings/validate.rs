//! Range checks and FMD2's empty-value fallbacks, applied to every update.

use std::net::SocketAddr;
use std::ops::RangeInclusive;

use super::model::{
    DEFAULT_CHAPTER_CUSTOMRENAME, DEFAULT_FILENAME_CUSTOMRENAME, DEFAULT_MANGA_CUSTOMRENAME,
    DEFAULT_PATH, DEFAULT_USER_AGENT, Settings,
};
use super::service::SettingsError;

/// Replaces blank values the way FMD2 does when it loads or applies options: the user agent
/// (mangadownloader/forms/frmMain.pas:6279-6285), the download directory and the rename
/// templates (mangadownloader/forms/frmMain.pas:5882-5917).
pub(super) fn normalize(s: &mut Settings) {
    fn reset_blank(value: &mut String, default: &str) {
        if value.trim().is_empty() {
            *value = default.to_string();
        }
    }
    let c = &mut s.connections;
    c.user_agent = c.user_agent.trim().to_string();
    reset_blank(&mut c.user_agent, DEFAULT_USER_AGENT);
    let saveto = &mut s.saveto;
    reset_blank(&mut saveto.default_dir, DEFAULT_PATH);
    reset_blank(&mut saveto.manga_rename, DEFAULT_MANGA_CUSTOMRENAME);
    reset_blank(&mut saveto.chapter_rename, DEFAULT_CHAPTER_CUSTOMRENAME);
    reset_blank(&mut saveto.filename_rename, DEFAULT_FILENAME_CUSTOMRENAME);
}

/// Checks every numeric setting against the range FMD2's spin edit allows (cited on each field
/// in `model.rs`) and the FMD2r-only settings against their own constraints.
pub(super) fn validate(s: &Settings) -> Result<(), SettingsError> {
    let c = &s.connections;
    check(
        "connections.max_parallel_tasks",
        c.max_parallel_tasks,
        1..=64,
    )?;
    check("connections.threads_per_task", c.threads_per_task, 1..=256)?;
    check("connections.retry_count", c.retry_count, -1..=5)?;
    check(
        "connections.auto_retry_failed_tasks",
        c.auto_retry_failed_tasks,
        0..=100,
    )?;
    check(
        "connections.max_favorite_threads",
        c.max_favorite_threads,
        1..=32,
    )?;
    check(
        "connections.max_update_list_threads",
        c.max_update_list_threads,
        1..=32,
    )?;
    check("connections.timeout_secs", c.timeout_secs, 1..=300)?;
    if c.proxy.port == Some(0) {
        return Err(invalid("connections.proxy.port", "must be 1..=65535"));
    }
    check(
        "saveto.digit_volume_length",
        s.saveto.digit_volume_length,
        1..=10,
    )?;
    check(
        "saveto.digit_chapter_length",
        s.saveto.digit_chapter_length,
        1..=10,
    )?;
    check("output.pdf_quality", s.output.pdf_quality, 5..=100)?;
    check("images.jpeg_quality", s.images.jpeg_quality, 1..=100)?;
    check(
        "images.imagemagick.quality",
        s.images.imagemagick.quality,
        1..=100,
    )?;
    check(
        "favorites.check_interval_minutes",
        s.favorites.check_interval_minutes,
        1..=1440,
    )?;
    check(
        "update_lists.interval_hours",
        s.update_lists.interval_hours,
        1..=u32::MAX,
    )?;
    check(
        "update_lists.new_manga_days",
        s.update_lists.new_manga_days,
        1..=365,
    )?;
    check(
        "module_updater.interval_minutes",
        s.module_updater.interval_minutes,
        1..=u32::MAX,
    )?;
    check("covers.cache_size_mb", s.covers.cache_size_mb, 1..=u32::MAX)?;
    if !c.flaresolverr_url.trim().is_empty()
        && super::websitebypass::flaresolverr_address(&c.flaresolverr_url).is_none()
    {
        return Err(invalid(
            "connections.flaresolverr_url",
            "must be empty or an http(s) URL such as http://flaresolverr:8191",
        ));
    }
    if s.server.bind.parse::<SocketAddr>().is_err() {
        return Err(invalid(
            "server.bind",
            "must be a socket address such as 0.0.0.0:8080",
        ));
    }
    Ok(())
}

fn check<T>(field: &str, value: T, range: RangeInclusive<T>) -> Result<(), SettingsError>
where
    T: PartialOrd + std::fmt::Display,
{
    if range.contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            &format!("{value} is outside {}..={}", range.start(), range.end()),
        ))
    }
}

pub(super) fn invalid(field: &str, reason: &str) -> SettingsError {
    SettingsError::Invalid {
        field: field.to_string(),
        reason: reason.to_string(),
    }
}
