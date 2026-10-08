//! `GET/PATCH /api/settings`, behind [`SettingsService`] so the typed settings model (T18) can
//! slot in without touching the handlers.

use axum::Json;
use axum::extract::State;
use fmd_store::AppDb;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

/// Errors a [`SettingsService`] reports.
#[derive(Debug, Error)]
pub enum SettingsError {
    /// The patch is malformed or sets an invalid value; maps to 400.
    #[error("invalid settings: {0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] fmd_store::StoreError),
}

impl From<SettingsError> for ApiError {
    fn from(err: SettingsError) -> Self {
        match err {
            SettingsError::Invalid(msg) => ApiError::BadRequest(msg),
            SettingsError::Store(e) => ApiError::Store(e),
        }
    }
}

/// Reads and updates the application settings as a JSON object. Methods block (they hit the
/// store); handlers call them on the blocking pool.
pub trait SettingsService: Send + Sync + 'static {
    /// All settings.
    fn get(&self) -> Result<Value, SettingsError>;
    /// Applies `patch` as a JSON merge patch (RFC 7396) and returns the resulting settings.
    fn patch(&self, patch: Value) -> Result<Value, SettingsError>;
}

/// Untyped settings kept as one JSON object under the `app` key of the `settings` table; the
/// stand-in until the typed model (T18) lands.
pub struct StoreSettings {
    db: AppDb,
}

const KEY: &str = "app";

impl StoreSettings {
    pub fn new(db: AppDb) -> Self {
        Self { db }
    }
}

impl SettingsService for StoreSettings {
    fn get(&self) -> Result<Value, SettingsError> {
        Ok(self
            .db
            .settings()
            .get(KEY)?
            .unwrap_or_else(|| Value::Object(Map::new())))
    }

    fn patch(&self, patch: Value) -> Result<Value, SettingsError> {
        if !patch.is_object() {
            return Err(SettingsError::Invalid("expected a JSON object".into()));
        }
        let mut settings = self.get()?;
        merge_patch(&mut settings, patch);
        self.db.settings().set(KEY, &settings)?;
        Ok(settings)
    }
}

/// RFC 7396 JSON merge patch: objects merge recursively, `null` removes, anything else replaces.
fn merge_patch(target: &mut Value, patch: Value) {
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
                merge_patch(target.entry(key).or_insert(Value::Null), value);
            }
        }
    }
}

/// All settings.
#[utoipa::path(get, path = "/api/settings", tag = "settings", operation_id = "getSettings",
    responses((status = 200, body = Object, description = "The settings object")))]
pub(crate) async fn get(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = state.settings.clone();
    Ok(Json(state.blocking(move |_| settings.get()).await?))
}

/// Update settings with a JSON merge patch (RFC 7396).
#[utoipa::path(patch, path = "/api/settings", tag = "settings", operation_id = "patchSettings",
    request_body(content = Object, content_type = "application/json"),
    responses(
        (status = 200, body = Object, description = "The updated settings object"),
        (status = 400, description = "Invalid patch", body = Problem),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    ApiJson(patch): ApiJson<Value>,
) -> Result<Json<Value>, ApiError> {
    let settings = state.settings.clone();
    Ok(Json(state.blocking(move |_| settings.patch(patch)).await?))
}
