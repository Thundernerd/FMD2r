//! Loading, validating, persisting and broadcasting [`Settings`].

use std::sync::{Arc, Mutex};

use fmd_store::{AppDb, StoreError};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::sync::watch;

use super::model::Settings;
use super::validate::{invalid, normalize, validate};

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// A value has the wrong type, is not one of an enum's values, or is out of range.
    #[error("invalid value for {field}: {reason}")]
    Invalid { field: String, reason: String },
    /// The patch names a setting that does not exist.
    #[error("unknown setting {0}")]
    UnknownKey(String),
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
    /// Reads the stored settings, filling every missing group or field with its default.
    ///
    /// A stored value this build cannot read (an enum value from a newer build, a wrong type)
    /// falls back to its default instead of failing the load; the group's other fields are kept.
    /// The unreadable value stays in the table until that group is next updated.
    pub fn load(db: AppDb) -> Result<Self, SettingsError> {
        let mut tree = serde_json::to_value(Settings::default())?;
        let keys: Vec<String> = tree
            .as_object()
            .map(|groups| groups.keys().cloned().collect())
            .unwrap_or_default();
        let repo = db.settings();
        for key in keys {
            if let Some(stored) = repo.get::<Value>(&key)? {
                overlay_group(&mut tree, &key, stored);
            }
        }
        let settings = Settings::deserialize(&tree)?;
        let (tx, _) = watch::channel(Arc::new(settings));
        Ok(Self {
            db,
            tx,
            write: Mutex::new(()),
        })
    }

    /// The current settings.
    pub fn get(&self) -> Arc<Settings> {
        self.tx.borrow().clone()
    }

    /// A receiver that sees every successful update that changed something, for live
    /// reconfiguration. Each update notifies once, however many settings it touched.
    pub fn subscribe(&self) -> watch::Receiver<Arc<Settings>> {
        self.tx.subscribe()
    }

    /// Applies `patch`, a JSON merge patch (RFC 7386) over the serialised [`Settings`]: objects
    /// merge key by key, other values replace, and `null` resets a setting to its default.
    ///
    /// The result is validated before anything is stored; on error nothing changes. Only the
    /// groups the patch changed are written, in one transaction, and subscribers are notified
    /// once. Blocking: call it from `spawn_blocking` in async code.
    pub fn update(&self, patch: Value) -> Result<Arc<Settings>, SettingsError> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let current = self.get();
        let mut tree = serde_json::to_value(&*current)?;
        check_known_keys(&tree, &patch, "")?;
        apply_merge_patch(&mut tree, patch);
        let mut next: Settings = serde_path_to_error::deserialize(tree)
            .map_err(|e| invalid(&e.path().to_string(), &e.inner().to_string()))?;
        normalize(&mut next);
        validate(&next)?;
        if next == *current {
            return Ok(current);
        }
        self.persist(&current, &next)?;
        let next = Arc::new(next);
        self.tx.send_replace(next.clone());
        Ok(next)
    }

    /// Writes the groups that differ between `old` and `new`. Each group is merged over what is
    /// stored, so keys this build does not know (written by a newer one) survive.
    fn persist(&self, old: &Settings, new: &Settings) -> Result<(), SettingsError> {
        let (Value::Object(old), Value::Object(new)) =
            (serde_json::to_value(old)?, serde_json::to_value(new)?)
        else {
            return Ok(());
        };
        let repo = self.db.settings();
        let mut changed = Vec::new();
        for (key, group) in new {
            if old.get(&key) == Some(&group) {
                continue;
            }
            let mut stored = repo.get::<Value>(&key)?.unwrap_or(Value::Null);
            merge(&mut stored, group);
            changed.push((key, stored));
        }
        let entries: Vec<(&str, &Value)> = changed.iter().map(|(k, v)| (k.as_str(), v)).collect();
        repo.set_many(&entries)?;
        Ok(())
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

/// Rejects patch keys that are not settings, so a typo is an error rather than a silent no-op.
fn check_known_keys(tree: &Value, patch: &Value, path: &str) -> Result<(), SettingsError> {
    let (Value::Object(tree), Value::Object(patch)) = (tree, patch) else {
        return Ok(());
    };
    for (key, value) in patch {
        let field = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        match tree.get(key) {
            Some(known) => check_known_keys(known, value, &field)?,
            None => return Err(SettingsError::UnknownKey(field)),
        }
    }
    Ok(())
}

/// RFC 7386 `MergePatch`.
fn apply_merge_patch(target: &mut Value, patch: Value) {
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
