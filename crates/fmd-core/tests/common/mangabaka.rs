//! MangaBaka's dump, served from the recorded records in tests/fixtures/mangabaka.

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fmd_core::metadata::{Download, DumpSource, MetadataError};
use fmd_http::TerminateToken;
use serde_json::Value;

/// The records of tests/fixtures/mangabaka/series.jsonl, in file order.
pub fn fixture_records() -> Vec<Value> {
    let path = format!(
        "{}/tests/fixtures/mangabaka/series.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// `records` as the dump ships them: JSON lines, zstd-compressed.
pub fn compress(records: &[Value]) -> Vec<u8> {
    let mut jsonl = String::new();
    for record in records {
        jsonl.push_str(&record.to_string());
        jsonl.push('\n');
    }
    zstd::encode_all(jsonl.as_bytes(), 3).unwrap()
}

/// Serves one compressed dump for every URL and records the URLs asked for.
pub struct FixtureSource {
    body: Mutex<Vec<u8>>,
    pub urls: Mutex<Vec<String>>,
    pub opened: AtomicUsize,
}

impl FixtureSource {
    pub fn new(records: &[Value]) -> Arc<Self> {
        Arc::new(Self {
            body: Mutex::new(compress(records)),
            urls: Mutex::default(),
            opened: AtomicUsize::new(0),
        })
    }

    pub fn set(&self, records: &[Value]) {
        *self.body.lock().unwrap() = compress(records);
    }
}

impl DumpSource for FixtureSource {
    fn open(&self, url: &str, _terminate: &TerminateToken) -> Result<Download, MetadataError> {
        self.urls.lock().unwrap().push(url.to_owned());
        self.opened.fetch_add(1, Ordering::SeqCst);
        let body = self.body.lock().unwrap().clone();
        let length = Some(body.len() as u64);
        let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(body));
        Ok(Download { reader, length })
    }
}
