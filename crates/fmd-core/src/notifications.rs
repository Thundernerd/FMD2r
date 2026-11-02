//! Pushing inbox events to ntfy, a Discord channel or any webhook. No FMD2 counterpart: FMD2
//! shows its balloon hints on the desktop only (baseunits/uDownloadsManager.pas:741-803).

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use fmd_store::EventSeverity;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::settings::Settings;

/// The most characters Discord takes across a message's Text Displays
/// (https://docs.discord.com/developers/components/reference, Text Display).
const DISCORD_TEXT_LIMIT: usize = 4000;
/// `IS_COMPONENTS_V2`: the message is laid out by `components` alone
/// (https://docs.discord.com/developers/resources/message, Message Flags).
const DISCORD_COMPONENTS_V2: u32 = 1 << 15;
/// A card's colour strip: Discord's blurple, yellow and red.
const DISCORD_INFO: u32 = 0x5865F2;
const DISCORD_WARNING: u32 = 0xF0B232;
const DISCORD_ERROR: u32 = 0xDA373C;

/// A service notifications are sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Ntfy,
    Discord,
    Webhook,
}

/// A kind of event a channel can be set to send: an inbox event kind, or an event sent without
/// one. Serialised as the kind, e.g. `download_failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// A library check found new chapters (`fmd_core::favorites`).
    NewChapters,
    /// A library check found chapters missing from the download folder.
    MissingChapters,
    /// A missing-chapter check that found nothing missing.
    MissingCheck,
    /// A completed series was removed from the library.
    Completed,
    /// A download failed (`fmd_server::notifications`).
    DownloadFailed,
    /// A download finished (`fmd_server::notifications`). Sent to the channels only, never to
    /// the inbox; off by default, as it fires for every task.
    DownloadCompleted,
    /// A series was added to the library (`fmd_server::favorites`). Sent to the channels only;
    /// off by default.
    SeriesAdded,
    /// A download was queued (`fmd_server::notifications`), by hand or by a library check. Sent
    /// to the channels only; off by default.
    DownloadQueued,
    /// A library check failed.
    FavoritesCheckFailed,
    /// Modules were updated, or updating them failed (`fmd_core::module_updater`).
    ModuleUpdate,
    /// Downloads were moved to another folder (`fmd_server::folder_move`).
    DownloadsMoved,
    /// The comic server was asked to rescan (`fmd_server::comic_server`).
    ComicServer,
    /// Downloads were held for a full disk, or resumed (`fmd_server::notifications`).
    DiskSpaceLow,
}

impl NotificationType {
    /// What "New chapters" used to send, and new channels send.
    pub const CHAPTERS: [NotificationType; 2] = [
        NotificationType::NewChapters,
        NotificationType::MissingChapters,
    ];

    /// What "Problems" used to send, and new channels send: every kind that could be a warning
    /// or an error. A missing-chapter check is one when it could not check some series
    /// (`fmd_core::favorites::add_failures`).
    pub const PROBLEMS: [NotificationType; 7] = [
        NotificationType::DownloadFailed,
        NotificationType::FavoritesCheckFailed,
        NotificationType::ModuleUpdate,
        NotificationType::DownloadsMoved,
        NotificationType::ComicServer,
        NotificationType::MissingCheck,
        NotificationType::DiskSpaceLow,
    ];

    /// The type of the inbox event kind `kind`; `None` for a kind no channel can pick, like a
    /// test notification.
    pub fn of_kind(kind: &str) -> Option<NotificationType> {
        serde_json::from_value(serde_json::Value::from(kind)).ok()
    }

    /// The name the settings show.
    pub fn label(self) -> &'static str {
        match self {
            NotificationType::NewChapters => "New chapters",
            NotificationType::MissingChapters => "Missing chapters",
            NotificationType::MissingCheck => "Missing-chapter check found nothing",
            NotificationType::Completed => "Completed series removed",
            NotificationType::DownloadFailed => "Download failed",
            NotificationType::DownloadCompleted => "Download completed",
            NotificationType::SeriesAdded => "Series added to library",
            NotificationType::DownloadQueued => "Download queued",
            NotificationType::FavoritesCheckFailed => "Library check failed",
            NotificationType::ModuleUpdate => "Module updates",
            NotificationType::DownloadsMoved => "Downloads moved",
            NotificationType::ComicServer => "Comic server rescan",
            NotificationType::DiskSpaceLow => "Disk almost full",
        }
    }

    /// The types a channel sends by default: [`Self::CHAPTERS`] and [`Self::PROBLEMS`].
    pub fn defaults() -> Vec<NotificationType> {
        [Self::CHAPTERS.as_slice(), Self::PROBLEMS.as_slice()].concat()
    }
}

/// What is sent: an inbox event, or a failure reported alongside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The inbox event kind, e.g. `new_chapters` or `download_failed`.
    pub kind: String,
    pub severity: EventSeverity,
    pub title: String,
    /// Plain text; it may span several lines.
    pub body: String,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// Labelled values, e.g. `("Website", "MangaDex")`, shown after the body.
    pub fields: Vec<(String, String)>,
    /// The series cover, which Discord shows beside the text.
    pub cover_url: Option<String>,
    /// The page of the web UI it is about, e.g. `/queue`, joined to `server.public_url`.
    pub link: Option<String>,
}

impl Notification {
    /// A warning or an error, which ntfy sends at high priority.
    pub fn is_problem(&self) -> bool {
        self.severity != EventSeverity::Info
    }

    /// The body and then the fields as `Label: value` lines: what the plain-text channels send.
    pub fn text(&self) -> String {
        let fields = self
            .fields
            .iter()
            .map(|(label, value)| format!("{label}: {value}"));
        Some(self.body.clone())
            .filter(|b| !b.is_empty())
            .into_iter()
            .chain(fields)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// An HTTP `POST` to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Ntfy, Channel::Discord, Channel::Webhook];

    /// The name the logs and the settings use.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Ntfy => "ntfy",
            Channel::Discord => "Discord",
            Channel::Webhook => "webhook",
        }
    }

    /// Whether the channel is on and ticks `notification`'s type.
    fn takes(self, settings: &Settings, notification: &Notification) -> bool {
        let n = &settings.notifications;
        let (enabled, types) = match self {
            Channel::Ntfy => (n.ntfy.enabled, &n.ntfy.types),
            Channel::Discord => (n.discord.enabled, &n.discord.types),
            Channel::Webhook => (n.webhook.enabled, &n.webhook.types),
        };
        enabled && NotificationType::of_kind(&notification.kind).is_some_and(|t| types.contains(&t))
    }
}

/// The request of every channel that is on, has a URL and ticks `notification`'s type.
pub fn requests(settings: &Settings, notification: &Notification) -> Vec<(Channel, Request)> {
    Channel::ALL
        .into_iter()
        .filter(|channel| channel.takes(settings, notification))
        .filter_map(|channel| Some((channel, request(channel, settings, notification)?)))
        .collect()
}

/// `channel`'s request for `notification`, whether or not the channel is on; `None` when it has
/// no URL.
pub fn request(
    channel: Channel,
    settings: &Settings,
    notification: &Notification,
) -> Option<Request> {
    match channel {
        Channel::Ntfy => ntfy(settings, notification),
        Channel::Discord => discord(settings, notification),
        Channel::Webhook => webhook(settings, notification),
    }
}

/// ntfy's publishing API (https://docs.ntfy.sh/publish/): the body is the message, the rest goes
/// in headers.
fn ntfy(settings: &Settings, notification: &Notification) -> Option<Request> {
    let ntfy = &settings.notifications.ntfy;
    let url = non_empty(&ntfy.topic_url)?;
    let mut headers = vec![("Title".into(), header_text(&notification.title))];
    if notification.is_problem() {
        headers.push(("Priority".into(), "4".into()));
    }
    if let Some(click) = non_empty(&settings.server.public_url) {
        headers.push(("Click".into(), click.to_owned()));
    }
    if !ntfy.token.is_empty() {
        headers.push(("Authorization".into(), format!("Bearer {}", ntfy.token)));
    }
    Some(Request {
        url: url.to_owned(),
        headers,
        body: notification.text(),
    })
}

/// Discord's Execute Webhook (https://docs.discord.com/developers/resources/webhook) with
/// Components V2 (https://docs.discord.com/developers/components/reference): a Container
/// coloured by severity holding the title, the body and the fields, the cover beside them when
/// there is one, and a link to FMD2r when its address is known. `with_components=true` makes a
/// webhook no app owns take components; such a webhook only takes non-interactive ones, so the
/// link is Markdown rather than a button.
fn discord(settings: &Settings, notification: &Notification) -> Option<Request> {
    let url = non_empty(&settings.notifications.discord.webhook_url)?;
    let separator = if url.contains('?') { '&' } else { '?' };
    let url = format!("{url}{separator}with_components=true");

    let title = format!("### {}", notification.title);
    let fields: Vec<String> = notification
        .fields
        .iter()
        .map(|(label, value)| format!("**{label}:** {value}"))
        .collect();
    let footer = non_empty(&settings.server.public_url).map(|public| {
        let page = notification.link.as_deref().unwrap_or_default();
        format!("-# [Open in FMD2r]({}{page})", public.trim_end_matches('/'))
    });
    let mut texts = vec![title];
    if !notification.body.is_empty() {
        texts.push(notification.body.clone());
    }
    if !fields.is_empty() {
        texts.push(fields.join("\n"));
    }
    let fixed = footer.as_ref().map_or(0, |f| f.chars().count());
    fit(&mut texts, DISCORD_TEXT_LIMIT.saturating_sub(fixed));

    let displays: Vec<serde_json::Value> = texts
        .into_iter()
        .map(|content| serde_json::json!({"type": 10, "content": content}))
        .collect();
    let mut components = match &notification.cover_url {
        // A Section takes one to three Text Displays and needs an accessory.
        Some(cover) => vec![serde_json::json!({
            "type": 9,
            "components": displays,
            "accessory": {"type": 11, "media": {"url": cover}},
        })],
        None => displays,
    };
    if let Some(footer) = footer {
        components.push(serde_json::json!({"type": 14}));
        components.push(serde_json::json!({"type": 10, "content": footer}));
    }
    let accent = match notification.severity {
        EventSeverity::Info => DISCORD_INFO,
        EventSeverity::Warning => DISCORD_WARNING,
        EventSeverity::Error => DISCORD_ERROR,
    };
    let body = serde_json::json!({
        "flags": DISCORD_COMPONENTS_V2,
        "components": [{"type": 17, "accent_color": accent, "components": components}],
    });
    Some(json_request(&url, Vec::new(), &body))
}

/// Cuts `texts` to `limit` characters in all, last text first, each cut one ending in "…".
fn fit(texts: &mut [String], limit: usize) {
    let mut total: usize = texts.iter().map(|t| t.chars().count()).sum();
    for text in texts.iter_mut().rev() {
        if total <= limit {
            return;
        }
        let length = text.chars().count();
        let keep = length.saturating_sub(total - limit).max(1);
        *text = cut(text, keep);
        total = total - length + text.chars().count();
    }
}

/// `names`, chapters in task order, as ranges: each run of names worded alike whose last number
/// goes up by at most 1 at a time (so part chapters like 12.5 stay in) becomes `first – last`,
/// e.g. `Chapter 0001 – 0047`. Names without a number stand alone. More than one chapter adds the
/// count, e.g. `Chapter 0001 – 0047 (47 chapters)`.
pub fn chapter_ranges<S: AsRef<str>>(names: &[S]) -> String {
    let mut runs: Vec<(&str, &str)> = Vec::new();
    let mut previous: Option<Numbered<'_>> = None;
    for name in names {
        let name = name.as_ref();
        let numbered = Numbered::parse(name);
        let extends = match (&previous, &numbered) {
            (Some(p), Some(n)) => {
                p.before == n.before && p.after == n.after && {
                    let step = n.value - p.value;
                    step > 0.0 && step <= 1.0
                }
            }
            _ => false,
        };
        match runs.last_mut() {
            Some(run) if extends => run.1 = name,
            _ => runs.push((name, name)),
        }
        previous = numbered;
    }
    let ranges: Vec<String> = runs
        .into_iter()
        .map(|(first, last)| {
            if first == last {
                return first.to_owned();
            }
            match Numbered::parse(last) {
                Some(n) if n.after.is_empty() => format!("{first} – {}", n.digits),
                _ => format!("{first} – {last}"),
            }
        })
        .collect();
    match names.len() {
        0 | 1 => ranges.join(", "),
        count => format!("{} ({count} chapters)", ranges.join(", ")),
    }
}

/// A chapter name split around its last number, e.g. `Ch. 12.5` into `Ch. `, `12.5` and ``.
struct Numbered<'a> {
    before: &'a str,
    digits: &'a str,
    after: &'a str,
    value: f64,
}

impl<'a> Numbered<'a> {
    fn parse(name: &'a str) -> Option<Numbered<'a>> {
        let bytes = name.as_bytes();
        let end = bytes.iter().rposition(u8::is_ascii_digit)? + 1;
        let digits_from = |end: usize| {
            bytes[..end]
                .iter()
                .rposition(|b| !b.is_ascii_digit())
                .map_or(0, |i| i + 1)
        };
        let mut start = digits_from(end);
        // A part chapter: `12.5`, not the `.5` of it.
        if start >= 2 && bytes[start - 1] == b'.' && bytes[start - 2].is_ascii_digit() {
            start = digits_from(start - 1);
        }
        let digits = &name[start..end];
        Some(Numbered {
            before: &name[..start],
            digits,
            after: &name[end..],
            value: digits.parse().ok()?,
        })
    }
}
/// Any endpoint taking JSON: the event's kind, severity, title, body and time.
fn webhook(settings: &Settings, notification: &Notification) -> Option<Request> {
    let webhook = &settings.notifications.webhook;
    let url = non_empty(&webhook.url)?;
    let mut headers = Vec::new();
    if !webhook.authorization.is_empty() {
        headers.push(("Authorization".into(), webhook.authorization.clone()));
    }
    let body = serde_json::json!({
        "kind": notification.kind,
        "severity": notification.severity.as_str(),
        "title": notification.title,
        "body": notification.text(),
        "created_at": notification.created_at,
    });
    Some(json_request(url, headers, &body))
}

/// `text` cut to `limit` characters, ending in "…" when it was longer.
fn cut(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        None => text.to_owned(),
        Some(_) => {
            let keep: String = text.chars().take(limit.saturating_sub(1)).collect();
            keep + "…"
        }
    }
}

fn json_request(
    url: &str,
    mut headers: Vec<(String, String)>,
    body: &serde_json::Value,
) -> Request {
    headers.push(("Content-Type".into(), "application/json".into()));
    Request {
        url: url.to_owned(),
        headers,
        body: body.to_string(),
    }
}

/// `value` trimmed, or `None` when that leaves nothing.
fn non_empty(value: &str) -> Option<&str> {
    Some(value.trim()).filter(|v| !v.is_empty())
}

/// `text` as a header value: as is when it is printable ASCII, else RFC 2047 Base64, which ntfy
/// decodes (https://docs.ntfy.sh/publish/, "UTF-8 in headers").
fn header_text(text: &str) -> String {
    if text.bytes().all(|b| (b' '..=b'~').contains(&b)) {
        text.to_owned()
    } else {
        format!("=?UTF-8?B?{}?=", BASE64.encode(text))
    }
}

/// Why a notification was not delivered. Never names the URL, which may carry a token.
#[derive(Debug, Error)]
pub enum SendError {
    /// The service answered with a status other than 2xx.
    #[error("HTTP {0}")]
    Status(u16),
    #[error("timed out")]
    TimedOut,
    #[error("header {0} is not a valid header value")]
    Header(String),
    #[error("{0}")]
    Failed(String),
}

/// Sends [`Request`]s once each, giving up after a timeout.
#[derive(Debug, Clone)]
pub struct Sender {
    client: reqwest::Client,
}

impl Sender {
    pub fn new(timeout: Duration) -> Result<Sender, SendError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| SendError::Failed(e.without_url().to_string()))?;
        Ok(Sender { client })
    }

    /// `POST`s `request`; no retries.
    pub async fn send(&self, request: &Request) -> Result<(), SendError> {
        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            let header = || SendError::Header(name.clone());
            headers.insert(
                HeaderName::try_from(name.as_str()).map_err(|_| header())?,
                HeaderValue::from_str(value).map_err(|_| header())?,
            );
        }
        let response = self
            .client
            .post(&request.url)
            .headers(headers)
            .body(request.body.clone())
            .send()
            .await
            .map_err(failure)?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(SendError::Status(status.as_u16()))
        }
    }
}

fn failure(e: reqwest::Error) -> SendError {
    if e.is_timeout() {
        SendError::TimedOut
    } else {
        SendError::Failed(e.without_url().to_string())
    }
}
