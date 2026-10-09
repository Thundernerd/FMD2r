//! `/api/accounts`: accounts of modules with `AccountSupport`
//! (mangadownloader/forms/frmAccountManager.pas). Passwords and cookies are write-only.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::accounts::{AccountError, AccountService, AccountUpdate, AccountView};
use fmd_store::AccountStatus;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

/// An account's status (`TAccountStatus`, baseunits/WebsiteModules.pas:78).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AccountState {
    Unknown,
    /// A login is running.
    Checking,
    Valid,
    Invalid,
}

impl From<AccountStatus> for AccountState {
    fn from(status: AccountStatus) -> Self {
        match status {
            AccountStatus::Unknown => Self::Unknown,
            AccountStatus::Checking => Self::Checking,
            AccountStatus::Valid => Self::Valid,
            AccountStatus::Invalid => Self::Invalid,
        }
    }
}

/// A module's account. The password is never returned; `has_password` says whether one is set.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AccountInfo {
    /// The module ID.
    pub module: String,
    /// The module's name.
    pub name: String,
    pub enabled: bool,
    pub username: String,
    pub has_password: bool,
    pub status: AccountState,
}

impl From<AccountView> for AccountInfo {
    fn from(view: AccountView) -> Self {
        Self {
            module: view.module_id,
            name: view.module_name,
            enabled: view.enabled,
            username: view.username,
            has_password: view.has_password,
            status: view.status.into(),
        }
    }
}

/// An account's status changed (`account.state`): a login started or finished.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AccountStateChange {
    pub module: String,
    pub status: AccountState,
}

/// The fields to change; a missing field keeps its value. Not `Debug`, so the password is never
/// logged.
#[derive(Deserialize, ToSchema)]
pub struct AccountRequest {
    pub username: Option<String>,
    /// Write-only.
    #[schema(format = Password)]
    pub password: Option<String>,
    pub enabled: Option<bool>,
}

impl From<AccountError> for ApiError {
    fn from(err: AccountError) -> Self {
        match err {
            AccountError::UnknownModule(_) | AccountError::NoAccountSupport(_) => {
                ApiError::NotFound
            }
            AccountError::NoLogin(_) | AccountError::Checking(_) => {
                ApiError::Conflict(err.to_string())
            }
            AccountError::Store(_) | AccountError::Pool(_) => ApiError::Internal(err.to_string()),
        }
    }
}

/// Runs `f` on the account service on its own thread, not tokio's blocking pool: the service
/// waits with `Pending::wait`, which refuses to run inside a runtime.
async fn with_service<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&AccountService) -> Result<T, AccountError> + Send + 'static,
) -> Result<T, ApiError> {
    let service = state.accounts.clone().ok_or(ApiError::NotFound)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("fmd-accounts".into())
        .spawn(move || {
            // The request may be gone.
            let _ = tx.send(f(&service));
        })
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    rx.await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(ApiError::from)
}

/// The accounts of every module with account support, by module ID.
#[utoipa::path(get, path = "/api/accounts", tag = "accounts", operation_id = "listAccounts",
    responses((status = 200, body = Vec<AccountInfo>, description = "The accounts")))]
pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountInfo>>, ApiError> {
    if state.accounts.is_none() {
        return Ok(Json(Vec::new()));
    }
    let accounts = with_service(&state, |s| Ok(s.list())).await?;
    Ok(Json(accounts.into_iter().map(AccountInfo::from).collect()))
}

/// Change a module's username, password or enabled flag. New credentials make the status
/// `unknown` until the next login. Turning the account on or off runs the module's
/// `OnAccountState`.
#[utoipa::path(put, path = "/api/accounts/{module}", tag = "accounts",
    operation_id = "putAccount",
    params(("module" = String, Path, description = "Module ID")),
    request_body = AccountRequest,
    responses(
        (status = 200, body = AccountInfo, description = "The updated account"),
        (status = 404, description = "No loaded module with account support has that ID",
            body = Problem),
        (status = 409, description = "A login of the account is running", body = Problem),
    ))]
pub(crate) async fn put(
    State(state): State<AppState>,
    Path(module): Path<String>,
    ApiJson(request): ApiJson<AccountRequest>,
) -> Result<Json<AccountInfo>, ApiError> {
    let update = AccountUpdate {
        username: request.username,
        password: request.password,
        enabled: request.enabled,
    };
    let view = with_service(&state, move |s| s.update(&module, update)).await?;
    Ok(Json(view.into()))
}

/// Clear a module's credentials and cookies and turn its account off.
#[utoipa::path(delete, path = "/api/accounts/{module}", tag = "accounts",
    operation_id = "deleteAccount",
    params(("module" = String, Path, description = "Module ID")),
    responses(
        (status = 204, description = "The account is cleared"),
        (status = 404, description = "No loaded module with account support has that ID",
            body = Problem),
        (status = 409, description = "A login of the account is running", body = Problem),
    ))]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(module): Path<String>,
) -> Result<StatusCode, ApiError> {
    with_service(&state, move |s| s.delete(&module)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Log in with the module's `OnLogin`, then `OnAccountState`; answers once done. `account.state`
/// events announce the start and the end.
#[utoipa::path(post, path = "/api/accounts/{module}/login", tag = "accounts",
    operation_id = "loginAccount",
    params(("module" = String, Path, description = "Module ID")),
    responses(
        (status = 200, body = AccountInfo, description = "The account after the login"),
        (status = 404, description = "No loaded module with account support has that ID",
            body = Problem),
        (status = 409, description = "The module has no login, or a login is already running",
            body = Problem),
    ))]
pub(crate) async fn login(
    State(state): State<AppState>,
    Path(module): Path<String>,
) -> Result<Json<AccountInfo>, ApiError> {
    let view = with_service(&state, move |s| s.login(&module)).await?;
    Ok(Json(view.into()))
}
