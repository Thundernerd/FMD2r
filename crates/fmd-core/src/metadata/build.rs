//! Streams MangaBaka's dump (`series.jsonl.zst`) into a [`MetadataBuilder`] line by line.

use std::io::{BufRead, BufReader, Read};

use fmd_http::TerminateToken;
use fmd_store::{MetadataBuilder, MetadataSeries};
use serde_json::{Map, Value};

use super::MetadataError;
use super::normalize::{link_key, person_keys, title_keys};

/// A missing (not just `null`) field means MangaBaka changed its schema.
const REQUIRED: &[&str] = &["id", "state"];
/// Fields matching reads from an active or merged series.
const REQUIRED_FOR_MATCHING: &[&str] = &[
    "title", "type", "titles", "authors", "artists", "links", "source",
];
/// Series between two progress reports and cancellation checks.
const REPORT_EVERY: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildProgress {
    /// Compressed.
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub series: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildSummary {
    pub series: u64,
    /// Merged series whose titles and IDs now point at another.
    pub merged: u64,
    /// Deleted or otherwise inactive.
    pub skipped: u64,
}

/// Counts the bytes read through it and stops when terminated.
pub(super) struct Counting<R> {
    pub(super) inner: R,
    pub(super) bytes: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub(super) terminate: TerminateToken,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.terminate.is_terminated() {
            // Not `Interrupted`: readers retry that, and would spin here forever.
            return Err(std::io::Error::other("cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.bytes
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

/// `progress` hears the series count every [`REPORT_EVERY`] series.
pub(super) fn read_dump(
    jsonl: impl Read,
    builder: &mut MetadataBuilder,
    terminate: &TerminateToken,
    mut progress: impl FnMut(u64),
) -> Result<BuildSummary, MetadataError> {
    let mut reader = BufReader::with_capacity(1 << 16, jsonl);
    let mut summary = BuildSummary::default();
    let mut line = String::new();
    let mut number = 0u64;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| read_error(e, terminate))?;
        if read == 0 {
            break;
        }
        number += 1;
        if line.trim().is_empty() {
            continue;
        }
        let record: Value = serde_json::from_str(&line).map_err(|source| MetadataError::Json {
            line: number,
            source,
        })?;
        let Some(record) = record.as_object() else {
            return Err(MetadataError::NotAnObject { line: number });
        };
        add_record(record, number, builder, &mut summary)?;
        let seen = summary.series + summary.merged + summary.skipped;
        if seen % REPORT_EVERY == 0 {
            if terminate.is_terminated() {
                return Err(MetadataError::Cancelled);
            }
            progress(seen);
        }
    }
    if terminate.is_terminated() {
        return Err(MetadataError::Cancelled);
    }
    if summary.series == 0 {
        return Err(MetadataError::Empty);
    }
    Ok(summary)
}

fn read_error(e: std::io::Error, terminate: &TerminateToken) -> MetadataError {
    if terminate.is_terminated() {
        MetadataError::Cancelled
    } else {
        MetadataError::Read(e)
    }
}

/// A merged series' titles, links and IDs point at the series it was merged into; other
/// inactive states are skipped.
fn add_record(
    record: &Map<String, Value>,
    line: u64,
    builder: &mut MetadataBuilder,
    summary: &mut BuildSummary,
) -> Result<(), MetadataError> {
    let id = record.get("id").and_then(Value::as_i64);
    let missing = |field: &str| MetadataError::MissingField {
        field: field.to_owned(),
        id,
        line,
    };
    for field in REQUIRED {
        if !record.contains_key(*field) {
            return Err(missing(field));
        }
    }
    let id = id.ok_or_else(|| MetadataError::BadField {
        field: "id".into(),
        line,
    })?;
    let state = str_field(record, "state");
    let merged_into = match state {
        "active" => None,
        "merged" => {
            let into = record
                .get("merged_with")
                .ok_or_else(|| missing("merged_with"))?;
            match into.as_i64() {
                Some(into) => Some(into),
                // Merged into nothing: nothing to point its titles at.
                None => {
                    summary.skipped += 1;
                    return Ok(());
                }
            }
        }
        _ => {
            summary.skipped += 1;
            return Ok(());
        }
    };
    for field in REQUIRED_FOR_MATCHING {
        if !record.contains_key(*field) {
            return Err(missing(field));
        }
    }

    for key in titles(record).iter().flat_map(|t| title_keys(t)) {
        builder.add_title(&key, id)?;
    }
    for link in strings(record.get("links")) {
        if let Some(key) = link_key(link) {
            builder.add_link(&key, id)?;
        }
    }
    if let Some(sources) = record.get("source").and_then(Value::as_object) {
        for (site, source) in sources {
            let xid = match source.get("id") {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => continue,
            };
            builder.add_xid(site, &xid, id)?;
        }
    }
    if let Some(into) = merged_into {
        builder.add_merge(id, into)?;
        summary.merged += 1;
        return Ok(());
    }

    let mut names: Vec<&str> = strings(record.get("authors")).collect();
    names.extend(strings(record.get("artists")));
    let cover = |size: &str| {
        record
            .get("cover")
            .and_then(|c| c.get(size))
            .and_then(|c| c.get("x1"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    builder.add_series(&MetadataSeries {
        id,
        title: str_field(record, "title").to_owned(),
        kind: str_field(record, "type").to_owned(),
        status: str_field(record, "status").to_owned(),
        year: record.get("year").and_then(Value::as_i64),
        content_rating: str_field(record, "content_rating").to_owned(),
        description: str_field(record, "description").to_owned(),
        genres: strings(record.get("genres")).map(str::to_owned).collect(),
        people: person_keys(&names.join(", ")).into_iter().collect(),
        cover_x150: cover("x150"),
        cover_x250: cover("x250"),
        cover_x350: cover("x350"),
    })?;
    summary.series += 1;
    Ok(())
}

fn titles(record: &Map<String, Value>) -> Vec<&str> {
    let mut out: Vec<&str> = ["title", "native_title", "romanized_title"]
        .into_iter()
        .map(|f| str_field(record, f))
        .collect();
    fn title_of(t: &Value) -> Option<&str> {
        t.get("title").and_then(Value::as_str)
    }
    if let Some(list) = record.get("titles").and_then(Value::as_array) {
        out.extend(list.iter().filter_map(title_of));
    }
    if let Some(groups) = record.get("secondary_titles").and_then(Value::as_object) {
        for group in groups.values().filter_map(Value::as_array) {
            out.extend(group.iter().filter_map(title_of));
        }
    }
    out.retain(|t| !t.is_empty());
    out
}

/// Empty when absent or not a string.
fn str_field<'a>(record: &'a Map<String, Value>, field: &str) -> &'a str {
    record
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn strings(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}
