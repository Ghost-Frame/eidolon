use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::UserIdentity;

#[derive(Debug, Deserialize)]
pub struct AuditQueryParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let audit_log = match &state.audit_log {
        Some(a) => a,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "audit logging is not enabled"})),
            ));
        }
    };

    let limit = params.limit.unwrap_or(50).min(500).max(1);
    let offset = params.offset.unwrap_or(0).max(0);

    match audit_log.query(&user.0, limit, offset).await {
        Ok(entries) => Ok(Json(json!({
            "ok": true,
            "entries": entries,
            "limit": limit,
            "offset": offset,
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("audit query failed: {}", e)})),
        )),
    }
}
