//! Loading, validating, persisting and broadcasting [`Settings`].

use std::sync::{Arc, Mutex};

use fmd_store::{AppDb, ListsDb, StoreError};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::sync::watch;

use super::model::{DEFAULT_DESTINATION_NAME, DEFAULT_PATH, Destination, Settings};
use super::module_overrides::{ModuleOverrides, encrypt_plain_module_secrets};
use super::password::{hash_patched, hash_stored};
use super::secrets::{Found, seal_group, unseal_group};
use super::validate::{normalize, sync_default_dir, validate, validate_destinations};
use crate::modules::OptionDef;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Every value of an update that has the wrong type, is not one of an enum's values, is
    /// out of range, or names a setting that does not exist. Never empty.
    #[error("{}", display_fields(.0))]
    Invalid(Vec<FieldError>),
    /// Hashing the server password failed (no randomness for the salt).
    #[error("hashing the password: {0}")]
    Hash(String),
}

/// One rejected value of an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// The setting, as a dotted path such as `connections.timeout_secs`.
    pub field: String,
    pub reason: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// The error for a setting that does not exist.
    pub fn unknown(field: impl Into<String>) -> Self {
        Self::new(field, "unknown setting")
    }
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value for {}: {}", self.field, self.reason)
    }
}

fn display_fields(errors: &[FieldError]) -> String {
    errors
        .iter()
        .map(FieldError::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

impl SettingsError {
    /// `errors` as an error, or `Ok` when there are none.
    pub(super) fn check(errors: Vec<FieldError>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self::Invalid(errors))
        }
    }
}

/// The one source of truth for application settings.
///
/// Each top-level group of [`Settings`] is stored as one JSON value in the `settings` table under
/// the group's name (`general`, `connections`, …).
pub struct SettingsService {
    db: AppDb,
    tx: watch::Sender<Arc<Settings>>,
    /// Serialises updates so two patches cannot both start from the same snapshot.
    write: Mutex<()>,
}

impl SettingsService {
    /// Reads the stored settings, defaulting every missing group or field.
    ///
    /// A stored value this build cannot read (e.g. an enum value from a newer build) falls back
    /// to its default rather than failing the load; it stays stored until the group is next
    /// updated. Plain secrets and an unhashed server password from older builds are
    /// encrypted/hashed here.
    pub fn load(db: AppDb) -> Result<Self, SettingsError> {
        let mut tree = serde_json::to_value(Settings::default())?;
        let keys: Vec<String> = tree
            .as_object()
            .map(|groups| groups.keys().cloned().collect())
            .unwrap_or_default();
        let repo = db.settings();
        for key in keys {
            if let Some(mut stored) = repo.get::<Value>(&key)? {
                if key == "server" && hash_stored(db.cipher(), &mut stored)? {
                    repo.set(&key, &stored)?;
                }
                if key == "saveto" && migrate_destinations(&mut stored) {
                    repo.set(&key, &stored)?;
                }
                if unseal_group(db.cipher(), &key, &mut stored) == Found::Plain {
                    let mut sealed = stored.clone();
                    seal_group(db.cipher(), &key, &mut sealed)?;
                    repo.set(&key, &sealed)?;
                }
                overlay_group(&mut tree, &key, stored);
            }
        }
        encrypt_plain_module_secrets(&db.module_settings())?;
        let mut settings = Settings::deserialize(&tree)?;
        settings.saveto.default_dir = settings.saveto.default_path().to_string();
        let (tx, _) = watch::channel(Arc::new(settings));
        Ok(Self {
            db,
            tx,
            write: Mutex::new(()),
        })
    }

    pub fn get(&self) -> Arc<Settings> {
        self.tx.borrow().clone()
    }

    /// Notified once per update that changed something, for live reconfiguration.
    pub fn subscribe(&self) -> watch::Receiver<Arc<Settings>> {
        self.tx.subscribe()
    }

    /// Applies `patch`, a JSON merge patch (RFC 7386) over the serialised [`Settings`]; `null`
    /// resets a setting to its default.
    ///
    /// On error nothing changes and the error lists every rejected value. Only changed groups
    /// are written, in one transaction. Blocking.
    pub fn update(&self, patch: Value) -> Result<Arc<Settings>, SettingsError> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let current = self.get();
        let next = Self::patched(&current, patch)?;
        if next == *current {
            return Ok(current);
        }
        self.persist(&current, &next)?;
        let next = Arc::new(next);
        self.tx.send_replace(next.clone());
        Ok(next)
    }

    /// On the first start with `general.selected_websites` (none stored yet), selects every
    /// module that has a list, so an upgraded install's Discover page keeps its websites.
    /// Blocking.
    pub fn select_listed_websites(&self, lists: &ListsDb) -> Result<(), SettingsError> {
        self.decide_on_first_start("selected_websites", || {
            let listed: Vec<String> = lists
                .masterlist()
                .summaries()?
                .into_iter()
                .filter(|summary| summary.count > 0)
                .map(|summary| summary.module_id)
                .collect();
            Ok(Value::from(listed))
        })
    }

    /// On the first start with `general.setup_completed` (none stored yet), marks an install
    /// that already has data (stored settings, library series, tasks or lists) as set up, so only
    /// a fresh install shows the setup wizard. Run it before anything else stores settings.
    /// Blocking.
    pub fn mark_existing_install_set_up(&self, lists: &ListsDb) -> Result<(), SettingsError> {
        self.decide_on_first_start("setup_completed", || Ok(Value::Bool(self.has_data(lists)?)))
    }

    /// Stores `general.<key>` as `decide` gives it, unless it is stored already.
    fn decide_on_first_start(
        &self,
        key: &str,
        decide: impl FnOnce() -> Result<Value, SettingsError>,
    ) -> Result<(), SettingsError> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let repo = self.db.settings();
        let mut stored = repo.get::<Value>("general")?.unwrap_or_default();
        if stored.get(key).is_some() {
            return Ok(());
        }
        let value = decide()?;
        // Stored even when it is the default, so later starts know this one ran.
        merge(&mut stored, serde_json::json!({ key: value.clone() }));
        repo.set("general", &stored)?;
        let current = self.get();
        let mut tree = serde_json::to_value(&*current)?;
        merge(&mut tree, serde_json::json!({ "general": { key: value } }));
        let next = Settings::deserialize(&tree)?;
        if next != *current {
            self.tx.send_replace(Arc::new(next));
        }
        Ok(())
    }

    /// Whether any settings group, library series, task or list is stored.
    fn has_data(&self, lists: &ListsDb) -> Result<bool, SettingsError> {
        let repo = self.db.settings();
        if let Value::Object(groups) = serde_json::to_value(Settings::default())? {
            for key in groups.keys() {
                if repo.get::<Value>(key)?.is_some() {
                    return Ok(true);
                }
            }
        }
        Ok(!self.db.favorites().list()?.is_empty()
            || !self.db.tasks().list()?.is_empty()
            || lists
                .masterlist()
                .summaries()?
                .iter()
                .any(|summary| summary.count > 0))
    }

    /// `current` with `patch` applied, normalised and validated.
    fn patched(current: &Settings, mut patch: Value) -> Result<Settings, SettingsError> {
        hash_patched(&mut patch)?;
        let mut errors = Vec::new();
        let next = merge_patch_reporting(serde_json::to_value(current)?, patch, &mut errors);
        let Some(mut next) = next else {
            return Err(SettingsError::Invalid(errors));
        };
        normalize(&mut next);
        errors.extend(validate_destinations(&current.saveto, &next.saveto));
        sync_default_dir(&current.saveto, &mut next.saveto);
        errors.extend(validate(&next));
        SettingsError::check(errors)?;
        Ok(next)
    }

    /// Applies `patch` like [`Self::update`] and each module's patch like
    /// [`ModuleOverrides::apply_patch`], all or nothing in one transaction. Rejected fields are
    /// prefixed `settings.` or `modules.<id>.`.
    pub fn update_with_modules(
        &self,
        patch: Value,
        modules: Vec<ModulePatch>,
    ) -> Result<(Arc<Settings>, Vec<ModuleOverrides>), SettingsError> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let current = self.get();
        let mut errors = Vec::new();
        let next = prefixed(Self::patched(&current, patch), "settings.", &mut errors)?;
        let repo = self.db.module_settings();
        let mut overrides = Vec::new();
        for module in &modules {
            let loaded = ModuleOverrides::load(&repo, &module.module_id)?;
            let patched = loaded.patched(&module.options, module.patch.clone());
            let prefix = format!("modules.{}.", module.module_id);
            if let Some(next) = prefixed(patched, &prefix, &mut errors)? {
                overrides.push(next);
            }
        }
        // Without errors every part parsed.
        let Some(next) = next.filter(|_| errors.is_empty()) else {
            return Err(SettingsError::Invalid(errors));
        };
        let groups = self.changed_groups(&current, &next)?;
        let stored = modules
            .iter()
            .zip(&overrides)
            .map(|(m, o)| o.merged_over_stored(&repo, &m.module_id))
            .collect::<Result<Vec<_>, _>>()?;
        let groups: Vec<(&str, &Value)> = groups.iter().map(|(k, v)| (k.as_str(), v)).collect();
        self.db.save_settings(&groups, &stored)?;
        let next = if next == *current {
            current
        } else {
            let next = Arc::new(next);
            self.tx.send_replace(next.clone());
            next
        };
        Ok((next, overrides))
    }

    /// Writes the groups that differ between `old` and `new`.
    fn persist(&self, old: &Settings, new: &Settings) -> Result<(), SettingsError> {
        let changed = self.changed_groups(old, new)?;
        let entries: Vec<(&str, &Value)> = changed.iter().map(|(k, v)| (k.as_str(), v)).collect();
        self.db.settings().set_many(&entries)?;
        Ok(())
    }

    /// The groups that differ between `old` and `new`, each merged over what is stored so keys
    /// unknown to this build survive.
    fn changed_groups(
        &self,
        old: &Settings,
        new: &Settings,
    ) -> Result<Vec<(String, Value)>, SettingsError> {
        let (Value::Object(old), Value::Object(new)) =
            (serde_json::to_value(old)?, serde_json::to_value(new)?)
        else {
            return Ok(Vec::new());
        };
        let repo = self.db.settings();
        let mut changed = Vec::new();
        for (key, group) in new {
            if old.get(&key) == Some(&group) {
                continue;
            }
            let mut stored = repo.get::<Value>(&key)?.unwrap_or(Value::Null);
            let undecided: Vec<&str> = FIRST_START_KEYS
                .iter()
                .filter(|(field, _)| key == "general" && stored.get(*field).is_none())
                .map(|(field, _)| *field)
                .collect();
            merge(&mut stored, group);
            // A default nobody chose stays unstored, so the first start still decides it.
            if let Value::Object(fields) = &mut stored {
                for (field, unchosen) in FIRST_START_KEYS {
                    if undecided.contains(&field) && fields.get(field).is_some_and(unchosen) {
                        fields.remove(field);
                    }
                }
            }
            seal_group(self.db.cipher(), &key, &mut stored)?;
            changed.push((key, stored));
        }
        Ok(changed)
    }
}

/// Whether a value is the default nobody chose.
type Unchosen = fn(&Value) -> bool;

/// The `general` settings the first start decides, each with a test for the default that stays
/// unstored until then: [`SettingsService::select_listed_websites`] and
/// [`SettingsService::mark_existing_install_set_up`].
const FIRST_START_KEYS: [(&str, Unchosen); 2] = [
    ("selected_websites", |v| {
        v.as_array().is_some_and(Vec::is_empty)
    }),
    ("setup_completed", |v| v == &Value::Bool(false)),
];

/// Turns the download folder a build before destinations stored (`default_dir`) into the
/// default destination, named [`DEFAULT_DESTINATION_NAME`]; `false` when `saveto` has
/// destinations already.
fn migrate_destinations(saveto: &mut Value) -> bool {
    let Value::Object(fields) = saveto else {
        return false;
    };
    if fields.contains_key("destinations") {
        return false;
    }
    let path = fields
        .get("default_dir")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .unwrap_or(DEFAULT_PATH)
        .to_string();
    let destination = Destination {
        name: DEFAULT_DESTINATION_NAME.into(),
        path,
        default: true,
    };
    fields.insert("destinations".into(), serde_json::json!([destination]));
    true
}

/// One module's part of [`SettingsService::update_with_modules`].
#[derive(Debug, Clone)]
pub struct ModulePatch {
    pub module_id: String,
    /// The options the module declares, which its option values are checked against.
    pub options: Vec<OptionDef>,
    /// A JSON merge patch over the module's [`ModuleOverrides`].
    pub patch: Value,
}

/// The value of `result`; its validation errors go to `errors` with their fields prefixed by
/// `prefix` (giving `None`), any other error is returned.
fn prefixed<T>(
    result: Result<T, SettingsError>,
    prefix: &str,
    errors: &mut Vec<FieldError>,
) -> Result<Option<T>, SettingsError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(SettingsError::Invalid(found)) => {
            errors.extend(found.into_iter().map(|e| FieldError {
                field: format!("{prefix}{}", e.field),
                reason: e.reason,
            }));
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Overlays the stored `group` onto `tree[key]`: whole if the result still parses as
/// [`Settings`], otherwise field by field, skipping each field that does not parse.
fn overlay_group(tree: &mut Value, key: &str, group: Value) {
    let try_overlay = |tree: &mut Value, patch: Value| {
        let mut candidate = tree.clone();
        if let Some(slot) = candidate.get_mut(key) {
            merge(slot, patch);
        }
        if Settings::deserialize(&candidate).is_ok() {
            *tree = candidate;
            true
        } else {
            false
        }
    };
    if try_overlay(tree, group.clone()) {
        return;
    }
    if let Value::Object(fields) = group {
        for (field, value) in fields {
            try_overlay(tree, Value::Object(Map::from_iter([(field, value)])));
        }
    }
}

/// Overlays `src` onto `dst`: objects merge key by key, anything else replaces.
pub(super) fn merge(dst: &mut Value, src: Value) {
    match (dst, src) {
        (Value::Object(dst), Value::Object(src)) => merge_maps(dst, src),
        (dst, src) => *dst = src,
    }
}

fn merge_maps(dst: &mut Map<String, Value>, src: Map<String, Value>) {
    for (key, value) in src {
        match dst.get_mut(&key) {
            Some(slot) => merge(slot, value),
            None => {
                dst.insert(key, value);
            }
        }
    }
}

/// Reports patch keys that are not settings, so a typo is an error rather than a silent no-op,
/// and drops them from `patch` so the rest of it can still be checked.
fn check_known_keys(tree: &Value, patch: &mut Value, path: &str, errors: &mut Vec<FieldError>) {
    let (Value::Object(tree), Value::Object(patch)) = (tree, patch) else {
        return;
    };
    patch.retain(|key, value| {
        let field = join_path(path, key);
        match tree.get(key) {
            Some(known) => {
                check_known_keys(known, value, &field, errors);
                true
            }
            None => {
                errors.push(FieldError::unknown(field));
                false
            }
        }
    });
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

/// `base` with the merge patch `patch` applied and deserialised, reporting every unknown key and
/// every value that does not parse; `None` when the result cannot be made to parse.
pub(super) fn merge_patch_reporting<T: serde::de::DeserializeOwned>(
    base: Value,
    mut patch: Value,
    errors: &mut Vec<FieldError>,
) -> Option<T> {
    check_known_keys(&base, &mut patch, "", errors);
    let mut tree = base.clone();
    apply_merge_patch(&mut tree, patch);
    deserialize_reporting(tree, &base, errors)
}

/// Deserialises `tree`, reporting every value that does not parse, not just the first: each is
/// reset to its value in `base` and the rest retried. `None` when a value cannot be reset.
fn deserialize_reporting<T: serde::de::DeserializeOwned>(
    mut tree: Value,
    base: &Value,
    errors: &mut Vec<FieldError>,
) -> Option<T> {
    loop {
        let err = match serde_path_to_error::deserialize::<_, T>(tree.clone()) {
            Ok(value) => return Some(value),
            Err(err) => err,
        };
        let keys: Vec<String> = err
            .path()
            .iter()
            .filter_map(|segment| match segment {
                serde_path_to_error::Segment::Map { key } => Some(key.clone()),
                _ => None,
            })
            .collect();
        errors.push(FieldError::new(
            err.path().to_string(),
            err.inner().to_string(),
        ));
        if !restore(&mut tree, base, &keys) {
            return None;
        }
    }
}

/// Puts the value at `keys` in `tree` back to the one in `base` (removing it when `base` has
/// none); `false` when that changes nothing.
fn restore(tree: &mut Value, base: &Value, keys: &[String]) -> bool {
    let Some((last, parents)) = keys.split_last() else {
        return false;
    };
    let mut node = tree;
    let mut original = Some(base);
    for key in parents {
        let Some(next) = node.get_mut(key) else {
            return false;
        };
        node = next;
        original = original.and_then(|o| o.get(key));
    }
    let Value::Object(map) = node else {
        return false;
    };
    match original.and_then(|o| o.get(last)) {
        Some(value) if map.get(last) != Some(value) => {
            map.insert(last.clone(), value.clone());
            true
        }
        Some(_) => false,
        None => map.remove(last).is_some(),
    }
}

/// RFC 7386 `MergePatch`.
pub(super) fn apply_merge_patch(target: &mut Value, patch: Value) {
    let Value::Object(patch) = patch else {
        *target = patch;
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    if let Value::Object(target) = target {
        for (key, value) in patch {
            if value.is_null() {
                target.remove(&key);
            } else {
                apply_merge_patch(target.entry(key).or_insert(Value::Null), value);
            }
        }
    }
}
