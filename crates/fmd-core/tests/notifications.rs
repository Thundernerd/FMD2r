//! Each notification channel's request for an inbox event (issues #268 and #299, "Seams under
//! test").
//!
//! Expected values: ntfy's publishing API (https://docs.ntfy.sh/publish/: `POST` to the topic,
//! `Title`/`Priority`/`Click` headers, `Authorization: Bearer <token>`, RFC 2047 for non-ASCII
//! headers), Discord's Execute Webhook with Components V2 (`flags` 32768, `with_components=true`,
//! at most 4000 characters of text) and the webhook payload the ticket specifies.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_core::notifications::{
    Channel, Notification, NotificationType, Request, chapter_ranges, request, requests,
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
        fields: Vec::new(),
        cover_url: None,
        link: None,
    }
}

fn download_failed() -> Notification {
    Notification {
        kind: "download_failed".into(),
        severity: EventSeverity::Error,
        title: "Download failed: One Piece".into(),
        body: "Ch. 1100: no pages".into(),
        created_at: "2026-10-13T09:00:00Z".into(),
        fields: Vec::new(),
        cover_url: None,
        link: None,
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

/// The one container of a Discord card: `body` must use Components V2.
fn card(request: &Request) -> Value {
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["flags"], 32768, "{body}");
    assert!(body.get("content").is_none(), "{body}");
    let components = body["components"].as_array().unwrap();
    assert_eq!(components.len(), 1, "{body}");
    assert_eq!(components[0]["type"], 17, "{body}");
    components[0].clone()
}

/// Every Text Display's content in `card`, in order, sections included.
fn texts(card: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    for component in card["components"].as_array().unwrap() {
        match component["type"].as_u64().unwrap() {
            10 => texts.push(component["content"].as_str().unwrap().to_owned()),
            9 => texts.extend(
                component["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["content"].as_str().unwrap().to_owned()),
            ),
            _ => {}
        }
    }
    texts
}

#[test]
fn discord_posts_a_components_card_with_the_title_and_the_body() {
    let request = request(Channel::Discord, &discord(), &new_chapters()).unwrap();
    // Webhooks not owned by an app ignore `components` without this.
    assert_eq!(
        request.url,
        "https://discord.com/api/webhooks/1/abc?with_components=true"
    );
    assert_eq!(header(&request, "Content-Type"), Some("application/json"));
    let card = card(&request);
    assert_eq!(
        texts(&card),
        [
            "### Found new chapter(s)",
            "Found 2 new chapter from 1 manga(s):\n- One Piece <MangaDex> has 2 new chapter(s).",
        ]
    );
}

#[test]
fn discord_keeps_a_query_the_webhook_url_has() {
    let settings = settings(json!({"notifications": {"discord": {
        "enabled": true, "webhook_url": "https://discord.com/api/webhooks/1/abc?thread_id=9"
    }}}));
    let request = request(Channel::Discord, &settings, &new_chapters()).unwrap();
    assert_eq!(
        request.url,
        "https://discord.com/api/webhooks/1/abc?thread_id=9&with_components=true"
    );
}

#[test]
fn discord_colours_the_card_by_severity() {
    let accent = |n: &Notification| {
        card(&request(Channel::Discord, &discord(), n).unwrap())["accent_color"]
            .as_u64()
            .unwrap()
    };
    let (info, warning, error) = (
        accent(&new_chapters()),
        accent(&module_update()),
        accent(&download_failed()),
    );
    assert_ne!(info, warning);
    assert_ne!(warning, error);
    assert_ne!(info, error);
}

fn download_completed() -> Notification {
    Notification {
        kind: "download_completed".into(),
        severity: EventSeverity::Info,
        title: "Download completed: One Piece".into(),
        body: String::new(),
        created_at: "2026-10-13T11:00:00Z".into(),
        fields: vec![
            (
                "Chapters".into(),
                "Chapter 0001 – 0047 (47 chapters)".into(),
            ),
            ("Website".into(), "MangaDex".into()),
        ],
        cover_url: Some("https://mangadex.org/covers/op.jpg".into()),
        link: Some("/queue".into()),
    }
}

#[test]
fn discord_shows_the_fields_beside_the_cover() {
    let request = request(Channel::Discord, &discord(), &download_completed()).unwrap();
    let card = card(&request);
    let section = &card["components"][0];
    assert_eq!(section["type"], 9, "{card}");
    assert_eq!(
        section["accessory"],
        json!({"type": 11, "media": {"url": "https://mangadex.org/covers/op.jpg"}})
    );
    assert_eq!(
        texts(&card),
        [
            "### Download completed: One Piece",
            "**Chapters:** Chapter 0001 – 0047 (47 chapters)\n**Website:** MangaDex",
        ]
    );
}

#[test]
fn discord_links_to_the_page_in_fmd2r_when_its_address_is_known() {
    let mut settings = discord();
    settings.server.public_url = "https://fmd.example.org/".into();
    let linked = card(&request(Channel::Discord, &settings, &download_completed()).unwrap());
    assert_eq!(
        texts(&linked).last().unwrap(),
        "-# [Open in FMD2r](https://fmd.example.org/queue)"
    );
    // Without a page, the link goes to FMD2r itself.
    let home = card(&request(Channel::Discord, &settings, &new_chapters()).unwrap());
    assert_eq!(
        texts(&home).last().unwrap(),
        "-# [Open in FMD2r](https://fmd.example.org)"
    );
}

/// Discord takes at most 4000 characters across a message's Text Displays.
#[test]
fn discord_cuts_long_text_to_4000_characters_ending_in_an_ellipsis() {
    let mut notification = new_chapters();
    // Multi-byte characters: the limit counts characters, not bytes.
    notification.body = "é".repeat(5000);
    let card = card(&request(Channel::Discord, &discord(), &notification).unwrap());
    let texts = texts(&card);
    let total: usize = texts.iter().map(|t| t.chars().count()).sum();
    assert_eq!(total, 4000);
    assert_eq!(texts[0], "### Found new chapter(s)");
    assert!(texts[1].ends_with("é…"));
}

#[test]
fn discord_leaves_text_of_exactly_4000_characters_whole() {
    let mut notification = new_chapters();
    let head = "### Found new chapter(s)".chars().count();
    notification.body = "x".repeat(4000 - head);
    let card = card(&request(Channel::Discord, &discord(), &notification).unwrap());
    assert!(texts(&card)[1].ends_with('x'));
}

/// The plain-text channels get the fields as `Label: value` lines after the body.
#[test]
fn ntfy_and_the_webhook_get_the_fields_as_lines() {
    let ntfy = request(Channel::Ntfy, &ntfy(), &download_completed()).unwrap();
    let lines = "Chapters: Chapter 0001 – 0047 (47 chapters)\nWebsite: MangaDex";
    assert_eq!(ntfy.body, lines);
    let settings = settings(json!({"notifications": {"webhook": {
        "enabled": true, "url": "http://n8n.local/webhook/fmd"
    }}}));
    let webhook = request(Channel::Webhook, &settings, &download_completed()).unwrap();
    let body: Value = serde_json::from_str(&webhook.body).unwrap();
    assert_eq!(body["body"], lines);
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
        fields: Vec::new(),
        cover_url: None,
        link: None,
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

/// "Series added" and "Download queued" fire for every series added and every task queued, so
/// a new channel leaves them off; ticking them sends them.
#[test]
fn series_added_and_download_queued_are_off_by_default_and_sent_when_ticked() {
    for kind in ["series_added", "download_queued"] {
        let mut notification = new_chapters();
        notification.kind = kind.into();
        assert_eq!(channels(&ntfy(), &notification), [], "{kind}");
        let settings = settings(json!({"notifications": {"ntfy": {
            "enabled": true, "topic_url": "https://ntfy.sh/t", "types": [kind],
        }}}));
        assert_eq!(
            channels(&settings, &notification),
            [Channel::Ntfy],
            "{kind}"
        );
    }
}

fn chapters(names: &[&str]) -> String {
    chapter_ranges(names)
}

#[test]
fn a_single_chapter_is_its_name() {
    assert_eq!(chapters(&["Chapter 0001"]), "Chapter 0001");
    assert_eq!(chapters(&[]), "");
}

#[test]
fn consecutive_chapters_become_a_range_keeping_their_padding() {
    let names: Vec<String> = (1..=47).map(|n| format!("Chapter {n:04}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(chapters(&names), "Chapter 0001 – 0047 (47 chapters)");
}

#[test]
fn gaps_split_the_ranges() {
    assert_eq!(
        chapters(&["Ch 1", "Ch 2", "Ch 3", "Ch 5", "Ch 7", "Ch 8", "Ch 9"]),
        "Ch 1 – 3, Ch 5, Ch 7 – 9 (7 chapters)"
    );
}

#[test]
fn part_chapters_stay_in_the_range() {
    assert_eq!(
        chapters(&["Ch. 12", "Ch. 12.5", "Ch. 13"]),
        "Ch. 12 – 13 (3 chapters)"
    );
}

/// Only the last number counts, and a range never spans two volumes or other wording.
#[test]
fn a_change_of_wording_starts_a_new_range() {
    assert_eq!(
        chapters(&["Vol.1 Ch.3", "Vol.1 Ch.4", "Vol.2 Ch.5", "Vol.2 Ch.6"]),
        "Vol.1 Ch.3 – 4, Vol.2 Ch.5 – 6 (4 chapters)"
    );
}

#[test]
fn chapters_without_a_number_stand_alone() {
    assert_eq!(
        chapters(&["Prologue", "Ch 1", "Ch 2", "Extra"]),
        "Prologue, Ch 1 – 2, Extra (4 chapters)"
    );
}

/// Text after the number shows in full at both ends.
#[test]
fn a_range_with_text_after_the_number_names_both_ends() {
    assert_eq!(
        chapters(&["Ch 1 (raw)", "Ch 2 (raw)"]),
        "Ch 1 (raw) – Ch 2 (raw) (2 chapters)"
    );
}
