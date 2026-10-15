//! Each notification channel's request for an inbox event (issues #268 and #299, "Seams under
//! test").
//!
//! Expected values: ntfy's publishing API (https://docs.ntfy.sh/publish/: `POST` to the topic,
//! `Title`/`Priority`/`Click` headers, `Authorization: Bearer <token>`, RFC 2047 for non-ASCII
//! headers), Discord's Execute Webhook (`content` of up to 2000 characters) and the webhook
//! payload the ticket specifies.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_core::notifications::{
    Channel, Notification, NotificationType, Request, request, requests,
};
use fmd_core::settings::Settings;
use fmd_store::EventSeverity;
use serde_json::{Value, json};

fn new_chapters() -> Notification {
    Notification {
        kind: "new_chapters".into(),
        severity: EventSeverity::Info,
        title: "Found new chapter(s)".into(),
        body: "Found 2 new chapter from 1 manga(s):\n- One Piece <MangaDex> has 2 new chapter(s)."
            .into(),
        created_at: "2026-10-13T08:00:00Z".into(),
    }
}

fn download_failed() -> Notification {
    Notification {
        kind: "download_failed".into(),
        severity: EventSeverity::Error,
        title: "Download failed: One Piece".into(),
        body: "Ch. 1100: no pages".into(),
        created_at: "2026-10-13T09:00:00Z".into(),
    }
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn settings(patch: Value) -> Settings {
    let mut tree = serde_json::to_value(Settings::default()).unwrap();
    merge(&mut tree, patch);
    serde_json::from_value(tree).unwrap()
}

fn merge(dst: &mut Value, src: Value) {
    match (dst, src) {
        (Value::Object(dst), Value::Object(src)) => {
            for (key, value) in src {
                merge(dst.entry(key).or_insert(Value::Null), value);
            }
        }
        (dst, src) => *dst = src,
    }
}

fn ntfy() -> Settings {
    settings(json!({"notifications": {"ntfy": {
        "enabled": true, "topic_url": "https://ntfy.sh/my-topic"
    }}}))
}

#[test]
fn ntfy_posts_the_body_to_the_topic_with_the_title_header() {
    let request = request(Channel::Ntfy, &ntfy(), &new_chapters()).unwrap();
    assert_eq!(request.url, "https://ntfy.sh/my-topic");
    assert_eq!(request.body, new_chapters().body);
    assert_eq!(header(&request, "Title"), Some("Found new chapter(s)"));
    assert_eq!(header(&request, "Priority"), None);
    assert_eq!(header(&request, "Authorization"), None);
    assert_eq!(header(&request, "Click"), None);
}

#[test]
fn ntfy_sends_problems_at_high_priority_and_the_token_when_set() {
    let settings = settings(json!({"notifications": {"ntfy": {
        "enabled": true, "topic_url": "https://ntfy.sh/my-topic", "token": "tk_secret"
    }}}));
    let request = request(Channel::Ntfy, &settings, &download_failed()).unwrap();
    assert_eq!(header(&request, "Priority"), Some("4"));
    assert_eq!(header(&request, "Authorization"), Some("Bearer tk_secret"));
}

#[test]
fn ntfy_links_to_fmd2r_when_its_address_is_known() {
    let mut settings = ntfy();
    settings.server.public_url = "https://fmd.example.org".into();
    let request = request(Channel::Ntfy, &settings, &new_chapters()).unwrap();
    assert_eq!(header(&request, "Click"), Some("https://fmd.example.org"));
}

#[test]
fn ntfy_encodes_a_title_that_is_not_ascii() {
    let mut notification = download_failed();
    notification.title = "Download failed: ワンピース".into();
    let request = request(Channel::Ntfy, &ntfy(), &notification).unwrap();
    // RFC 2047 Base64 of the UTF-8 title.
    assert_eq!(
        header(&request, "Title"),
        Some("=?UTF-8?B?RG93bmxvYWQgZmFpbGVkOiDjg6/jg7Pjg5Tjg7zjgrk=?=")
    );
}

fn discord() -> Settings {
    settings(json!({"notifications": {"discord": {
        "enabled": true, "webhook_url": "https://discord.com/api/webhooks/1/abc"
    }}}))
}

#[test]
fn discord_posts_the_bold_title_and_the_body_as_content() {
    let request = request(Channel::Discord, &discord(), &new_chapters()).unwrap();
    assert_eq!(request.url, "https://discord.com/api/webhooks/1/abc");
    assert_eq!(header(&request, "Content-Type"), Some("application/json"));
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(
        body,
        json!({"content": "**Found new chapter(s)**\nFound 2 new chapter from 1 manga(s):\n- One Piece <MangaDex> has 2 new chapter(s)."})
    );
}

#[test]
fn discord_cuts_long_text_to_2000_characters_ending_in_an_ellipsis() {
    let mut notification = new_chapters();
    // Multi-byte characters: the limit counts characters, not bytes.
    notification.body = "é".repeat(3000);
    let request = request(Channel::Discord, &discord(), &notification).unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    let content = body["content"].as_str().unwrap();
    assert_eq!(content.chars().count(), 2000);
    assert!(content.starts_with("**Found new chapter(s)**\néé"));
    assert!(content.ends_with("é…"));
}

#[test]
fn discord_leaves_text_of_exactly_2000_characters_whole() {
    let mut notification = new_chapters();
    let head = "**Found new chapter(s)**\n".chars().count();
    notification.body = "x".repeat(2000 - head);
    let request = request(Channel::Discord, &discord(), &notification).unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    let content = body["content"].as_str().unwrap();
    assert_eq!(content.chars().count(), 2000);
    assert!(content.ends_with('x'));
}

#[test]
fn the_webhook_posts_the_event_as_json_with_the_authorization_when_set() {
    let settings = settings(json!({"notifications": {"webhook": {
        "enabled": true, "url": "http://n8n.local/webhook/fmd", "authorization": "Bearer hook"
    }}}));
    let request = request(Channel::Webhook, &settings, &download_failed()).unwrap();
    assert_eq!(request.url, "http://n8n.local/webhook/fmd");
    assert_eq!(header(&request, "Content-Type"), Some("application/json"));
    assert_eq!(header(&request, "Authorization"), Some("Bearer hook"));
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(
        body,
        json!({
            "kind": "download_failed",
            "severity": "error",
            "title": "Download failed: One Piece",
            "body": "Ch. 1100: no pages",
            "created_at": "2026-10-13T09:00:00Z",
        })
    );
}

#[test]
fn the_webhook_sends_no_authorization_when_none_is_set() {
    let settings = settings(json!({"notifications": {"webhook": {
        "enabled": true, "url": "http://n8n.local/webhook/fmd"
    }}}));
    let request = request(Channel::Webhook, &settings, &new_chapters()).unwrap();
    assert_eq!(header(&request, "Authorization"), None);
}

fn channels(settings: &Settings, notification: &Notification) -> Vec<Channel> {
    requests(settings, notification)
        .into_iter()
        .map(|(channel, _)| channel)
        .collect()
}

#[test]
fn only_channels_that_are_on_and_tick_the_events_type_are_sent_to() {
    let settings = settings(json!({"notifications": {
        "ntfy": {"enabled": true, "topic_url": "https://ntfy.sh/t"},
        "discord": {"enabled": true, "webhook_url": "https://discord.com/api/webhooks/1/abc",
                    "types": ["new_chapters", "missing_chapters"]},
        "webhook": {"enabled": false, "url": "http://n8n.local/webhook/fmd"},
    }}));
    assert_eq!(
        channels(&settings, &new_chapters()),
        [Channel::Ntfy, Channel::Discord]
    );
    assert_eq!(channels(&settings, &download_failed()), [Channel::Ntfy]);
}

#[test]
fn a_channel_that_is_off_is_not_sent_to_whatever_it_ticks() {
    let settings = settings(json!({"notifications": {"webhook": {
        "enabled": false, "url": "http://n8n.local/webhook/fmd",
        "types": ["new_chapters", "download_failed"],
    }}}));
    assert_eq!(channels(&settings, &new_chapters()), []);
    assert_eq!(channels(&settings, &download_failed()), []);
}

#[test]
fn a_channel_on_without_a_url_is_not_sent_to() {
    let settings = settings(json!({"notifications": {"ntfy": {"enabled": true}}}));
    assert_eq!(channels(&settings, &new_chapters()), []);
}

fn module_update() -> Notification {
    Notification {
        kind: "module_update".into(),
        severity: EventSeverity::Warning,
        title: "Module update failed".into(),
        body: "MangaDex: checksum mismatch".into(),
        created_at: "2026-10-13T10:00:00Z".into(),
    }
}

/// "Problems" used to send every warning; now a channel picks the problems it wants.
#[test]
fn unticking_module_updates_stops_a_module_warning() {
    let mut settings = ntfy();
    assert_eq!(channels(&settings, &module_update()), [Channel::Ntfy]);
    settings
        .notifications
        .ntfy
        .types
        .retain(|t| *t != NotificationType::ModuleUpdate);
    assert_eq!(channels(&settings, &module_update()), []);
    assert_eq!(channels(&settings, &download_failed()), [Channel::Ntfy]);
}

/// Info events are sent when their type is ticked, e.g. a finished folder move; a kind with no
/// type, like a test notification, goes to no channel.
#[test]
fn any_ticked_type_is_sent_whatever_its_severity() {
    let settings = settings(json!({"notifications": {"ntfy": {
        "enabled": true, "topic_url": "https://ntfy.sh/t",
        "types": ["downloads_moved", "completed"],
    }}}));
    let mut moved = new_chapters();
    moved.kind = "downloads_moved".into();
    assert_eq!(channels(&settings, &moved), [Channel::Ntfy]);
    let mut completed = new_chapters();
    completed.kind = "completed".into();
    assert_eq!(channels(&settings, &completed), [Channel::Ntfy]);
    assert_eq!(channels(&settings, &new_chapters()), []);
    let mut test = new_chapters();
    test.kind = "test".into();
    assert_eq!(channels(&Settings::default(), &test), []);
}

/// New channels take what "New chapters" and "Problems" took before: new and missing chapters
/// and every kind that can be a problem (a missing-chapter check that could not check some
/// series is a warning), but no other news.
#[test]
fn a_new_channel_takes_chapters_and_problems() {
    let settings = ntfy();
    let mut missing = new_chapters();
    missing.kind = "missing_chapters".into();
    assert_eq!(channels(&settings, &missing), [Channel::Ntfy]);
    for kind in [
        "download_failed",
        "favorites_check_failed",
        "module_update",
        "downloads_moved",
        "comic_server",
        "missing_check",
        "disk_space_low",
    ] {
        let mut problem = download_failed();
        problem.kind = kind.into();
        assert_eq!(channels(&settings, &problem), [Channel::Ntfy], "{kind}");
    }
    let mut completed = new_chapters();
    completed.kind = "completed".into();
    assert_eq!(channels(&settings, &completed), []);
}

/// "Download completed" fires for every finished task, including auto-downloaded new chapters,
/// so a new channel leaves it off (issue #300); ticking it sends it.
#[test]
fn download_completed_is_off_by_default_and_sent_when_ticked() {
    let mut completed = new_chapters();
    completed.kind = "download_completed".into();
    assert_eq!(channels(&ntfy(), &completed), []);
    let settings = settings(json!({"notifications": {"ntfy": {
        "enabled": true, "topic_url": "https://ntfy.sh/t", "types": ["download_completed"],
    }}}));
    assert_eq!(channels(&settings, &completed), [Channel::Ntfy]);
}
