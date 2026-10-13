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

/// The most characters Discord takes in a message's `content`
/// (https://docs.discord.com/developers/resources/webhook, Execute Webhook).
const DISCORD_CONTENT_LIMIT: usize = 2000;

/// The inbox event kinds of a library check's finds (`fmd_core::favorites`).
const NEW_CHAPTER_KINDS: [&str; 2] = ["new_chapters", "missing_chapters"];

/// A service notifications are sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Ntfy,
    Discord,
    Webhook,
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
}

impl Notification {
    /// A warning or an error, sent to the channels that take "Problems".
    pub fn is_problem(&self) -> bool {
        self.severity != EventSeverity::Info
    }

    /// New or missing chapters found by a library check, sent to the channels that take "New
    /// chapters".
    pub fn is_new_chapters(&self) -> bool {
        !self.is_problem() && NEW_CHAPTER_KINDS.contains(&self.kind.as_str())
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

    /// Whether the channel is on and takes `notification`'s kind of event.
    fn takes(self, settings: &Settings, notification: &Notification) -> bool {
        let n = &settings.notifications;
        let (enabled, new_chapters, problems) = match self {
            Channel::Ntfy => (n.ntfy.enabled, n.ntfy.new_chapters, n.ntfy.problems),
            Channel::Discord => (
                n.discord.enabled,
                n.discord.new_chapters,
                n.discord.problems,
            ),
            Channel::Webhook => (
                n.webhook.enabled,
                n.webhook.new_chapters,
                n.webhook.problems,
            ),
        };
        enabled
            && ((problems && notification.is_problem())
                || (new_chapters && notification.is_new_chapters()))
    }
}

/// The request of every channel that is on, has a URL and takes `notification`'s kind of event.
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
        body: notification.body.clone(),
    })
}

/// Discord's Execute Webhook (https://docs.discord.com/developers/resources/webhook): the title
/// in bold over the body, as `content`.
fn discord(settings: &Settings, notification: &Notification) -> Option<Request> {
    let url = non_empty(&settings.notifications.discord.webhook_url)?;
    let content = cut(
        &format!("**{}**\n{}", notification.title, notification.body),
        DISCORD_CONTENT_LIMIT,
    );
    Some(json_request(
        url,
        Vec::new(),
        &serde_json::json!({ "content": content }),
    ))
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
        "body": notification.body,
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
