//! `/api/library/groups`: named sections of the library that fold away. A favorite is in at most
//! one; the rest show under "Ungrouped".

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::favorites::auto_group_library;
use fmd_core::metadata::FORMATS;
use fmd_store::{AppDb, GroupRule, LibraryGroup, LibraryGroupId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::favorites::null_or_value;
use crate::{ApiError, AppState, Problem};

/// A library group, as the Library's sections show it.
#[derive(Debug, Serialize, ToSchema)]
pub struct GroupView {
    pub id: i64,
    pub name: String,
    /// Whether the Library shows the section folded.
    pub collapsed: bool,
    /// Which series in no group it collects; `null` is filled by hand only.
    pub rule: Option<RuleView>,
}

impl From<LibraryGroup> for GroupView {
    fn from(group: LibraryGroup) -> Self {
        Self {
            id: group.id.0,
            name: group.name,
            collapsed: group.collapsed,
            rule: group.rule.map(RuleView::from),
        }
    }
}

/// What an auto group collects: series in no group with the tag (website and MangaBaka genres,
/// whole and ignoring case), or whose MangaBaka match has the format. The first group in order
/// that matches a series gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RuleView {
    pub kind: RuleKind,
    /// The tag, or one of `manga`, `manhwa`, `manhua`, `oel` and `other`.
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Tag,
    Format,
}

impl From<GroupRule> for RuleView {
    fn from(rule: GroupRule) -> Self {
        match rule {
            GroupRule::Tag(value) => Self {
                kind: RuleKind::Tag,
                value,
            },
            GroupRule::Format(value) => Self {
                kind: RuleKind::Format,
                value,
            },
        }
    }
}

/// A group to add.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewGroup {
    /// Unique, ignoring case.
    pub name: String,
    /// Which series in no group it collects; missing or `null` is filled by hand only.
    #[serde(default)]
    pub rule: Option<RuleView>,
}

/// Fields of a group to change; missing ones stay.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupPatch {
    pub name: Option<String>,
    pub collapsed: Option<bool>,
    /// Which series in no group it collects; `null` clears it.
    #[serde(default, deserialize_with = "null_or_value")]
    #[schema(value_type = Option<RuleView>, nullable)]
    pub rule: Option<Option<RuleView>>,
}

/// The groups' new order.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupOrder {
    /// These first, in this order; the rest keep their relative order.
    pub ids: Vec<i64>,
}

/// The library's groups in display order.
#[utoipa::path(get, path = "/api/library/groups", tag = "library", operation_id = "listGroups",
    responses((status = 200, body = Vec<GroupView>)))]
pub(crate) async fn list(State(state): State<AppState>) -> Result<Json<Vec<GroupView>>, ApiError> {
    let groups = state
        .blocking(|db| -> Result<_, ApiError> { Ok(db.library_groups().list()?) })
        .await?;
    Ok(Json(groups.into_iter().map(GroupView::from).collect()))
}

/// Add a group after the others. With a rule, it collects the matching series in no group.
#[utoipa::path(post, path = "/api/library/groups", tag = "library", operation_id = "createGroup",
    request_body = NewGroup,
    responses(
        (status = 201, body = GroupView),
        (status = 400, body = Problem, description = "The name or the rule's tag is empty, or the rule's format is unknown"),
        (status = 409, body = Problem, description = "Another group has the name, ignoring case"),
    ))]
pub(crate) async fn create(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<NewGroup>,
) -> Result<(StatusCode, Json<GroupView>), ApiError> {
    let name = group_name(&req.name)?;
    let rule = req.rule.map(group_rule).transpose()?;
    let metadata = state.metadata.clone();
    let group = state
        .blocking(move |db| -> Result<_, ApiError> {
            ensure_free(db, &name, None)?;
            let mut group = db.library_groups().create(&name)?;
            if rule.is_some() {
                group.rule = rule;
                db.library_groups().update(&group)?;
                auto_group_library(db, metadata.as_ref())?;
            }
            Ok(group)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(group.into())))
}

/// Rename a group, fold or unfold its section, or set or clear its rule. A rule set collects the
/// matching series in no group.
#[utoipa::path(patch, path = "/api/library/groups/{id}", tag = "library",
    operation_id = "updateGroup",
    params(("id" = i64, Path, description = "Group id")),
    request_body = GroupPatch,
    responses(
        (status = 200, body = GroupView),
        (status = 400, body = Problem, description = "The name or the rule's tag is empty, or the rule's format is unknown"),
        (status = 404, body = Problem, description = "No such group"),
        (status = 409, body = Problem, description = "Another group has the name, ignoring case"),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(req): ApiJson<GroupPatch>,
) -> Result<Json<GroupView>, ApiError> {
    let name = req.name.as_deref().map(group_name).transpose()?;
    let rule = req
        .rule
        .map(|r| r.map(group_rule).transpose())
        .transpose()?;
    let metadata = state.metadata.clone();
    let group = state
        .blocking(move |db| -> Result<_, ApiError> {
            let id = LibraryGroupId(id);
            let mut group = db.library_groups().get(id)?.ok_or(ApiError::NotFound)?;
            if let Some(name) = name {
                ensure_free(db, &name, Some(id))?;
                group.name = name;
            }
            if let Some(collapsed) = req.collapsed {
                group.collapsed = collapsed;
            }
            let collect = rule.as_ref().is_some_and(Option::is_some);
            if let Some(rule) = rule {
                group.rule = rule;
            }
            db.library_groups().update(&group)?;
            if collect {
                auto_group_library(db, metadata.as_ref())?;
            }
            Ok(group)
        })
        .await?;
    Ok(Json(group.into()))
}

/// Remove a group; its series stay in the library, ungrouped.
#[utoipa::path(delete, path = "/api/library/groups/{id}", tag = "library",
    operation_id = "deleteGroup",
    params(("id" = i64, Path, description = "Group id")),
    responses(
        (status = 204, description = "Removed"),
        (status = 404, body = Problem, description = "No such group"),
    ))]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .blocking(move |db| -> Result<(), ApiError> {
            let id = LibraryGroupId(id);
            db.library_groups().get(id)?.ok_or(ApiError::NotFound)?;
            db.library_groups().delete(id)?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reorder the groups; the series in no group go to the first group whose rule matches them.
#[utoipa::path(post, path = "/api/library/groups/reorder", tag = "library",
    operation_id = "reorderGroups",
    request_body = GroupOrder,
    responses((status = 204, description = "Reordered")))]
pub(crate) async fn reorder(
    State(state): State<AppState>,
    ApiJson(order): ApiJson<GroupOrder>,
) -> Result<StatusCode, ApiError> {
    let metadata = state.metadata.clone();
    state
        .blocking(move |db| -> Result<(), ApiError> {
            let ids: Vec<_> = order.ids.into_iter().map(LibraryGroupId).collect();
            db.library_groups().reorder(&ids)?;
            auto_group_library(db, metadata.as_ref())?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The rule with its value trimmed; an empty tag or an unknown format is a 400.
fn group_rule(rule: RuleView) -> Result<GroupRule, ApiError> {
    let value = rule.value.trim().to_owned();
    match rule.kind {
        RuleKind::Tag if value.is_empty() => Err(ApiError::BadRequest(
            "the rule's tag cannot be empty".into(),
        )),
        RuleKind::Tag => Ok(GroupRule::Tag(value)),
        RuleKind::Format if FORMATS.contains(&value.as_str()) => Ok(GroupRule::Format(value)),
        RuleKind::Format => Err(ApiError::BadRequest(format!(
            "{value:?} is not a format; one of {}",
            FORMATS.join(", ")
        ))),
    }
}

/// The trimmed name; empty is a 400.
fn group_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "the group name cannot be empty".into(),
        ));
    }
    Ok(name.to_owned())
}

/// 409 when a group other than `own` has `name`, ignoring case.
fn ensure_free(db: &AppDb, name: &str, own: Option<LibraryGroupId>) -> Result<(), ApiError> {
    match db.library_groups().find(name)? {
        Some(other) if Some(other.id) != own => Err(ApiError::Conflict(format!(
            "a group named {} exists already",
            other.name
        ))),
        _ => Ok(()),
    }
}
