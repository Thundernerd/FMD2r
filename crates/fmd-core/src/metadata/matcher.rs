//! Matching list titles against MangaBaka's database offline, by T71's rules
//! (docs/research/metadata-sources.md, "Matching"; `match_dump` and `decide` in
//! docs/research/metadata-probe/probe.py): the site link, then cross-site IDs, then the title
//! and the people.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use fmd_http::{HttpClient, TerminateToken};
use fmd_store::{ListsDb, MatchConfidence, MatchInput, MetadataSeries, StoredMatch};
use regex::Regex;
use serde_json::Value;

use super::normalize::{link_key, people_agree, person_keys, title_keys};
use super::{Metadata, MetadataError, USER_AGENT};

/// A title matching more series than this ("Love", "Blue") is too generic to tell them apart.
const MAX_TITLE_CANDIDATES: usize = 50;
/// Titles per MangaDex request (`ids[]`), MangaDex's page limit.
const MANGADEX_BATCH: usize = 100;
/// The time between two MangaDex requests: under its ~5 requests a second
/// (https://api.mangadex.org/docs/2-limitations/).
const MANGADEX_INTERVAL: Duration = Duration::from_millis(250);
/// Matches stored per transaction.
const STORE_BATCH: usize = 500;
/// MangaDex's `attributes.links` keys and the MangaBaka `source` sites they name.
const MANGADEX_LINKS: [(&str, &str); 5] = [
    ("al", "anilist"),
    ("mu", "manga_updates"),
    ("mal", "my_anime_list"),
    ("kt", "kitsu"),
    ("ap", "anime_planet"),
];

static MANGADEX_UUID: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"title/([0-9a-fA-F-]{36})").ok());

/// A module whose list is matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListModule {
    pub id: String,
    /// The module's `RootURL`, which its list's links are relative to.
    pub root_url: String,
}

/// Which titles of a list to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchScope {
    /// Titles not matched yet, or changed since: after a list update or FMD2-DB import.
    Changed,
    /// Every title: after the database was refreshed, since MangaBaka's titles and IDs change
    /// too.
    All,
}

/// What matching a list did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchReport {
    /// Titles matched.
    pub examined: u64,
    /// Of those, titles with an accepted match.
    pub accepted: u64,
    /// Whether it was terminated before matching every title.
    pub cancelled: bool,
}

/// Reads the cross-site IDs of MangaDex titles from MangaDex's API.
pub struct MangaDexLinks {
    http: HttpClient,
    last: std::sync::Mutex<Option<Instant>>,
}

impl MangaDexLinks {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            last: std::sync::Mutex::new(None),
        }
    }

    /// The MangaBaka `(site, id)` pairs MangaDex links each title of `uuids` to: one
    /// `GET /manga?ids[]=...` per 100 titles, spaced to stay under MangaDex's rate limit. After a
    /// request fails, the rest are not asked and count as unanswered.
    fn links(&self, uuids: &[String], terminate: &TerminateToken) -> CrossIds {
        let mut out = CrossIds::default();
        let mut batches = uuids.chunks(MANGADEX_BATCH);
        for batch in batches.by_ref() {
            if terminate.is_terminated() {
                out.unanswered.extend(batch.iter().cloned());
                break;
            }
            match self.batch(batch, terminate) {
                Ok(links) => out.links.extend(links),
                Err(e) => {
                    tracing::warn!(target: "fmd_core", "MangaDex cross-site IDs: {e}");
                    out.unanswered.extend(batch.iter().cloned());
                    break;
                }
            }
        }
        out.unanswered.extend(batches.flatten().cloned());
        out
    }

    /// The links of one batch of at most 100 titles, by UUID.
    fn batch(
        &self,
        batch: &[String],
        terminate: &TerminateToken,
    ) -> Result<HashMap<String, Vec<(String, String)>>, MetadataError> {
        self.wait(terminate);
        let mut url = format!("https://api.mangadex.org/manga?limit={MANGADEX_BATCH}");
        for rating in ["safe", "suggestive", "erotica", "pornographic"] {
            url.push_str("&contentRating[]=");
            url.push_str(rating);
        }
        for uuid in batch {
            url.push_str("&ids[]=");
            url.push_str(uuid);
        }
        let mut http = self.http.session();
        http.set_terminate_token(terminate.clone());
        http.set_user_agent(USER_AGENT);
        http.headers_mut().set_value("Accept", "application/json");
        let ok = http
            .get(&url)
            .map_err(|e| MetadataError::Http(e.to_string()))?;
        let code = http.result_code();
        if !ok || code >= 300 || terminate.is_terminated() {
            return Err(MetadataError::Download {
                url,
                status: u16::try_from(code).unwrap_or(0),
            });
        }
        let body: Value = serde_json::from_slice(http.document())
            .map_err(|e| MetadataError::Http(format!("MangaDex's answer: {e}")))?;
        let mut out = HashMap::new();
        for manga in body["data"].as_array().into_iter().flatten() {
            let Some(id) = manga["id"].as_str() else {
                continue;
            };
            // MangaDex sends `[]` (or `null`) for a title without links.
            let Some(links) = manga["attributes"]["links"].as_object() else {
                continue;
            };
            let pairs = MANGADEX_LINKS
                .iter()
                .filter_map(|(key, site)| {
                    let xid = match links.get(*key)? {
                        Value::String(s) if !s.is_empty() => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => return None,
                    };
                    Some(((*site).to_owned(), xid))
                })
                .collect();
            out.insert(id.to_lowercase(), pairs);
        }
        Ok(out)
    }

    /// Waits until [`MANGADEX_INTERVAL`] has passed since the last request.
    fn wait(&self, terminate: &TerminateToken) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = *last {
            let due = at + MANGADEX_INTERVAL;
            while Instant::now() < due && !terminate.is_terminated() {
                std::thread::sleep((due - Instant::now()).min(Duration::from_millis(50)));
            }
        }
        *last = Some(Instant::now());
    }
}

/// Matches the titles of `lists.db` against MangaBaka's database and stores the results there.
pub struct Matcher {
    lists: ListsDb,
    mangadex: MangaDexLinks,
}

/// What MangaDex said about a list's titles.
#[derive(Default)]
struct CrossIds {
    /// The MangaBaka `(site, id)` pairs of each title MangaDex answered for, by UUID.
    links: HashMap<String, Vec<(String, String)>>,
    /// The titles MangaDex could not be asked about.
    unanswered: HashSet<String>,
}

impl Matcher {
    pub fn new(lists: ListsDb, mangadex: MangaDexLinks) -> Self {
        Self { lists, mangadex }
    }

    /// Matches the titles of `module`'s list that `scope` picks against `meta`, and stores one
    /// match per title. Blocks (MangaDex requests), so call it from a thread outside any tokio
    /// runtime.
    pub fn match_module(
        &self,
        meta: &Metadata,
        module: &ListModule,
        scope: MatchScope,
        terminate: &TerminateToken,
    ) -> Result<MatchReport, MetadataError> {
        let matches = self.lists.matches();
        matches.prune(&module.id).map_err(MetadataError::Lists)?;
        let build_id = meta.build_id()?;
        let mut inputs = match scope {
            MatchScope::Changed => matches.pending(&module.id, &build_id),
            MatchScope::All => matches.all(&module.id, &build_id),
        }
        .map_err(MetadataError::Lists)?;
        let cross_ids = if is_mangadex(&module.root_url) {
            let uuids: Vec<String> = inputs
                .iter()
                .filter_map(|i| mangadex_uuid(&i.link))
                .collect();
            self.mangadex.links(&uuids, terminate)
        } else {
            CrossIds::default()
        };
        // A title MangaDex could not be asked about still matches by its title now, and is
        // stored as changed, so the next run asks again.
        for input in &mut inputs {
            if mangadex_uuid(&input.link).is_some_and(|u| cross_ids.unanswered.contains(&u)) {
                input.fingerprint.clear();
            }
        }

        let mut report = MatchReport::default();
        for batch in inputs.chunks(STORE_BATCH) {
            if terminate.is_terminated() {
                report.cancelled = true;
                break;
            }
            let decided = batch
                .iter()
                .map(|input| {
                    let xids = mangadex_uuid(&input.link)
                        .and_then(|uuid| cross_ids.links.get(&uuid))
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    decide(meta, module, input, xids)
                })
                .collect::<Result<Vec<_>, _>>()?;
            report.examined += decided.len() as u64;
            report.accepted += decided
                .iter()
                .filter(|m| m.confidence.is_accepted())
                .count() as u64;
            matches
                .store(&module.id, batch.iter().zip(&decided))
                .map_err(MetadataError::Lists)?;
        }
        Ok(report)
    }
}

/// The match of one list title: its site link, then its cross-site IDs, then its titles and
/// people.
fn decide(
    meta: &Metadata,
    module: &ListModule,
    input: &MatchInput,
    xids: &[(String, String)],
) -> Result<StoredMatch, MetadataError> {
    if let Some(key) = link_key(&absolute(&module.root_url, &input.link)) {
        let ids = meta.by_link_key(&key)?;
        if let [id] = ids.as_slice()
            && let Some(series) = meta.series(*id)?
        {
            return Ok(accepted(MatchConfidence::Link, &series));
        }
    }
    if !xids.is_empty() {
        let mut ids = BTreeSet::new();
        for (site, xid) in xids {
            ids.extend(meta.find_by_xid(site, xid)?);
        }
        if ids.len() == 1
            && let Some(id) = ids.first()
            && let Some(series) = meta.series(*id)?
        {
            return Ok(accepted(MatchConfidence::CrossId, &series));
        }
    }

    let want: HashSet<String> = entry_titles(input)
        .iter()
        .flat_map(|t| title_keys(t))
        .collect();
    let mut ids = BTreeSet::new();
    for key in &want {
        ids.extend(meta.by_title_key(key)?);
    }
    if ids.len() > MAX_TITLE_CANDIDATES {
        return Ok(rejected(MatchConfidence::Ambiguous));
    }
    let mut candidates = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(series) = meta.series(id)? {
            candidates.push(series);
        }
    }
    Ok(pick(input, candidates, &want))
}

/// T71's `decide`: the candidate the title and people point at.
fn pick(
    input: &MatchInput,
    candidates: Vec<MetadataSeries>,
    want: &HashSet<String>,
) -> StoredMatch {
    // Lists hold comics; a novel's entry is not the comic's.
    let hits: Vec<MetadataSeries> = candidates
        .into_iter()
        .filter(|c| c.kind != "novel")
        .collect();
    if hits.is_empty() {
        return rejected(MatchConfidence::None);
    }
    let people = person_keys(&format!("{}, {}", input.authors, input.artists));
    let (pool, confidence) = if people.is_empty() {
        (hits, MatchConfidence::TitleUnique)
    } else {
        let agree: Vec<MetadataSeries> = hits
            .into_iter()
            .filter(|c| people_agree(&c.people, &people))
            .collect();
        if agree.is_empty() {
            return rejected(MatchConfidence::AuthorConflict);
        }
        (agree, MatchConfidence::TitleAuthor)
    };
    match narrow(pool, want).as_slice() {
        [only] => accepted(confidence, only),
        _ => rejected(MatchConfidence::Ambiguous),
    }
}

/// Ties go to the candidates whose main title matches.
fn narrow(candidates: Vec<MetadataSeries>, want: &HashSet<String>) -> Vec<MetadataSeries> {
    let (main, rest): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|c| title_keys(&c.title).iter().any(|k| want.contains(k)));
    if main.is_empty() { rest } else { main }
}

/// The title and its alt titles. Sites separate alt titles differently (MangaDex ", ",
/// MangaFire "; ", Asura Scans " • "); the strongest separator present splits them, since
/// titles themselves contain commas.
fn entry_titles(input: &MatchInput) -> Vec<&str> {
    let alts = input.alttitles.as_str();
    let sep = ["\n", "•", ";"]
        .into_iter()
        .find(|s| alts.contains(s))
        .unwrap_or(",");
    std::iter::once(input.title.as_str())
        .chain(alts.split(sep).map(str::trim))
        .filter(|t| !t.is_empty())
        .collect()
}

fn accepted(confidence: MatchConfidence, series: &MetadataSeries) -> StoredMatch {
    StoredMatch {
        series_id: Some(series.id),
        confidence,
        format: Some(format_of(&series.kind).to_owned()),
        publication: publication_of(&series.status).map(str::to_owned),
        year: series.year,
    }
}

fn rejected(confidence: MatchConfidence) -> StoredMatch {
    StoredMatch {
        series_id: None,
        confidence,
        format: None,
        publication: None,
        year: None,
    }
}

/// Discover's format facet value for MangaBaka's `type`.
fn format_of(kind: &str) -> &'static str {
    match kind {
        "manga" => "manga",
        "manhwa" => "manhwa",
        "manhua" => "manhua",
        "oel" => "oel",
        _ => "other",
    }
}

/// Discover's publication facet value for MangaBaka's `status`; `None` when it does not say.
fn publication_of(status: &str) -> Option<&'static str> {
    match status {
        "releasing" => Some("ongoing"),
        "completed" => Some("completed"),
        "hiatus" => Some("hiatus"),
        "cancelled" => Some("cancelled"),
        _ => None,
    }
}

/// `link` made absolute against `root`, like `MaybeFillHost` (baseunits/uBaseUnit.pas).
fn absolute(root: &str, link: &str) -> String {
    if link.contains("://") {
        link.to_owned()
    } else if let Some(path) = link.strip_prefix('/') {
        format!("{}/{path}", root.trim_end_matches('/'))
    } else {
        format!("{}/{link}", root.trim_end_matches('/'))
    }
}

/// Whether `root_url` is MangaDex's.
fn is_mangadex(root_url: &str) -> bool {
    let host = root_url
        .split("://")
        .nth(1)
        .unwrap_or(root_url)
        .split(['/', ':'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    host == "mangadex.org" || host.ends_with(".mangadex.org")
}

/// The MangaDex title UUID in a MangaDex list link (`title/<uuid>`).
fn mangadex_uuid(link: &str) -> Option<String> {
    let caps = MANGADEX_UUID.as_ref()?.captures(link)?;
    Some(caps.get(1)?.as_str().to_lowercase())
}
