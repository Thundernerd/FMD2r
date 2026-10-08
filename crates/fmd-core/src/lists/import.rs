//! Bootstrapping a module's list from FMD2-DB's prebuilt dumps, as FMD2's `TDBUpdaterThread`
//! does (baseunits/DBUpdater.pas:110-202): download `<module id>.7z`, extract the
//! `<module id>.db` it holds, and make its rows the module's list.

use std::io::{Cursor, Read, Write};

use fmd_http::{HttpClient, HttpError, TerminateToken};
use fmd_store::{ListsDb, StoreError, read_fmd2_list};
use sevenz_rust::{Password, SevenZReader};
use thiserror::Error;

/// The largest database extracted from a dump. The biggest FMD2-DB dumps hold a few hundred
/// thousand titles, well under this.
const MAX_DB_BYTES: u64 = 2 << 30;

/// Why an FMD2-DB import failed. The module's list is left as it was.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("HTTP: {0}")]
    Http(#[from] HttpError),
    /// The download did not succeed (`HTTP.GET(...) and (HTTP.ResultCode < 300)`,
    /// baseunits/DBUpdater.pas:125).
    #[error("downloading {url} failed with HTTP status {status}")]
    Download { url: String, status: i32 },
    #[error("the download was cancelled")]
    Cancelled,
    #[error("reading the 7z archive: {0}")]
    Archive(String),
    #[error("the archive holds no .db file")]
    NoDatabase,
    #[error("the database in the archive is larger than {MAX_DB_BYTES} bytes")]
    TooLarge,
    #[error("writing the extracted database: {0}")]
    Io(#[from] std::io::Error),
    #[error("lists.db or the extracted database: {0}")]
    Store(#[from] StoreError),
}

/// `GetDBURL` (baseunits/DBUpdater.pas:56-63): `<website>` in `template` (any case) replaced
/// by the module ID. FMD2's fallback that appends the ID never runs, as its guard
/// `Pos(...) <> -1` is always true (`Pos` returns 0 when nothing is found), so a template
/// without `<website>` is used as it is.
pub fn db_url(template: &str, module_id: &str) -> String {
    const PLACEHOLDER: &str = "<website>";
    let lower = template.to_ascii_lowercase();
    let mut url = String::with_capacity(template.len() + module_id.len());
    let mut rest = 0;
    for (at, _) in lower.match_indices(PLACEHOLDER) {
        url.push_str(&template[rest..at]);
        url.push_str(module_id);
        rest = at + PLACEHOLDER.len();
    }
    url.push_str(&template[rest..]);
    url
}

/// Imports FMD2-DB dumps into `lists.db`.
pub struct DbImporter {
    http: HttpClient,
    lists: ListsDb,
}

impl DbImporter {
    pub fn new(http: HttpClient, lists: ListsDb) -> Self {
        Self { http, lists }
    }

    /// Downloads module `module_id`'s dump from [`db_url`]`(url_template, module_id)` and
    /// imports it with [`DbImporter::import_archive`]. Blocks, so call it from a thread outside
    /// any tokio runtime. `status` hears each step, as FMD2's status bar shows it
    /// (`RS_Downloading`, `RS_Extracting`, baseunits/DBUpdater.pas:123, :166). Returns the
    /// number of titles imported.
    pub fn import(
        &self,
        module_id: &str,
        url_template: &str,
        terminate: &TerminateToken,
        status: &mut dyn FnMut(&str),
    ) -> Result<u64, ImportError> {
        let url = db_url(url_template, module_id);
        status("Downloading...");
        let mut http = self.http.session();
        http.set_terminate_token(terminate.clone());
        let ok = http.get(&url)?;
        if terminate.is_terminated() {
            return Err(ImportError::Cancelled);
        }
        let code = http.result_code();
        if !ok || code >= 300 {
            return Err(ImportError::Download { url, status: code });
        }
        status("Extracting...");
        self.import_archive(module_id, http.document())
    }

    /// Makes the rows of the FMD2 database in the 7z `archive` the list of module
    /// `module_id`, replacing what it listed, as FMD2 overwrites `<module id>.db` with the
    /// extracted one (baseunits/DBUpdater.pas:157-177). The archive's `<module id>.db` is
    /// used, or else its first `.db` file. Returns the number of titles imported.
    pub fn import_archive(&self, module_id: &str, archive: &[u8]) -> Result<u64, ImportError> {
        let db = extract_db(module_id, archive)?;
        let rows = read_fmd2_list(db.path())?;
        let masterlist = self.lists.masterlist();
        masterlist.replace_module(module_id, &rows)?;
        Ok(masterlist.count(Some(module_id))?)
    }
}

/// Extracts the database of `archive` to a temporary file.
fn extract_db(module_id: &str, archive: &[u8]) -> Result<tempfile::NamedTempFile, ImportError> {
    let len = archive.len() as u64;
    let archive_error = |e: sevenz_rust::Error| ImportError::Archive(e.to_string());
    let mut reader =
        SevenZReader::new(Cursor::new(archive), len, Password::empty()).map_err(archive_error)?;
    let wanted = format!("{module_id}.db");
    let is_db = |name: &str| name.to_ascii_lowercase().ends_with(".db");
    let names: Vec<String> = reader
        .archive()
        .files
        .iter()
        .filter(|e| !e.is_directory() && is_db(e.name()))
        .map(|e| e.name().to_owned())
        .collect();
    let base = |name: &str| name.rsplit(['/', '\\']).next().unwrap_or(name).to_owned();
    let chosen = names
        .iter()
        .find(|n| base(n).eq_ignore_ascii_case(&wanted))
        .or_else(|| names.first())
        .cloned()
        .ok_or(ImportError::NoDatabase)?;

    let mut file = tempfile::NamedTempFile::new()?;
    let mut failure = None;
    reader
        .for_each_entries(|entry, data| {
            if entry.name() != chosen {
                return Ok(true);
            }
            let copied = std::io::copy(&mut data.take(MAX_DB_BYTES + 1), file.as_file_mut());
            match copied {
                Ok(n) if n > MAX_DB_BYTES => failure = Some(ImportError::TooLarge),
                Ok(_) => {}
                Err(e) => failure = Some(ImportError::Io(e)),
            }
            Ok(false)
        })
        .map_err(archive_error)?;
    if let Some(failure) = failure {
        return Err(failure);
    }
    file.as_file_mut().flush()?;
    Ok(file)
}
