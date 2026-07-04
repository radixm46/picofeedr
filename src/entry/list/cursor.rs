use crate::cli::SortOrder;
use crate::error::{AppError, error_details};
use crate::query::{EntryQuery, FeedFilter};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha1::{Digest, Sha1};

/// Cursor payload for pagination.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) k: i64,
    pub(super) id: i64,
    sort: String,
    query_hash: String,
}

/// Encodes pagination cursor with query metadata.
pub(super) fn encode_cursor_with_query(
    key: i64,
    id: i64,
    sort: SortOrder,
    query_hash: &str,
) -> Result<String, AppError> {
    let cursor = Cursor {
        k: key,
        id,
        sort: sort.as_str().to_string(),
        query_hash: query_hash.to_string(),
    };
    let bytes = serde_json::to_vec(&cursor)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Decodes and validates pagination cursor.
pub(super) fn decode_cursor(
    raw: &str,
    sort: SortOrder,
    query_hash: &str,
) -> Result<Cursor, AppError> {
    let bytes = URL_SAFE_NO_PAD.decode(raw.as_bytes()).map_err(|error| {
        AppError::invalid_query_with_details(
            format!("Invalid cursor: {error}"),
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("base64url_decode_failed")),
            ]),
        )
    })?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::invalid_query_with_details(
            format!("Invalid cursor: {error}"),
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("cursor_json_decode_failed")),
            ]),
        )
    })?;
    if cursor.sort != sort.as_str() || cursor.query_hash != query_hash {
        return Err(AppError::invalid_query_with_details(
            "Cursor does not match the current query",
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("cursor_mismatch")),
            ]),
        ));
    }
    Ok(cursor)
}

/// Computes a stable hash for query validation.
pub(super) fn compute_query_hash(query: &EntryQuery) -> String {
    let mut components = Vec::new();
    if let Some(tag_expr) = &query.tag_expr {
        components.push(format!("tag_expr={}", tag_expr.canonical()));
    }
    if let Some(feed) = &query.feed {
        match feed {
            FeedFilter::Id(id) => components.push(format!("feed_id={id}")),
            FeedFilter::Title(title) => components.push(format!("feed_title={title}")),
        }
    }
    if !query.title_terms.is_empty() {
        let mut terms = query
            .title_terms
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        terms.sort_unstable();
        components.push(format!(
            "title_terms={}",
            serde_json::to_string(&terms).expect("serialize title terms")
        ));
    }
    if !query.negated_title_terms.is_empty() {
        let mut terms = query
            .negated_title_terms
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        terms.sort_unstable();
        components.push(format!(
            "negated_title_terms={}",
            serde_json::to_string(&terms).expect("serialize negated title terms")
        ));
    }
    if !query.term_groups.is_empty() {
        let mut groups = query
            .term_groups
            .iter()
            .map(|expr| expr.canonical())
            .collect::<Vec<_>>();
        groups.sort_unstable();
        components.push(format!(
            "term_groups={}",
            serde_json::to_string(&groups).expect("serialize term groups")
        ));
    }
    if !query.negated_term_groups.is_empty() {
        let mut groups = query
            .negated_term_groups
            .iter()
            .map(|expr| expr.canonical())
            .collect::<Vec<_>>();
        groups.sort_unstable();
        components.push(format!(
            "negated_term_groups={}",
            serde_json::to_string(&groups).expect("serialize negated term groups")
        ));
    }
    if let Some(after) = query.after {
        components.push(format!("after={after}"));
    }
    if let Some(before) = query.before {
        components.push(format!("before={before}"));
    }
    let payload = components.join("|");
    let mut hasher = Sha1::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}
