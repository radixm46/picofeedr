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

#[derive(Serialize)]
struct QueryHashPayload {
    tag_expr: Option<String>,
    feed: Option<QueryHashFeed>,
    title_terms: Vec<String>,
    negated_title_terms: Vec<String>,
    term_groups: Vec<String>,
    negated_term_groups: Vec<String>,
    after: Option<i64>,
    before: Option<i64>,
}

#[derive(Serialize)]
enum QueryHashFeed {
    Id(String),
    Title(String),
}

/// Computes a stable hash for query validation.
pub(super) fn compute_query_hash(query: &EntryQuery) -> String {
    let mut title_terms = query.title_terms.clone();
    title_terms.sort_unstable();
    let mut negated_title_terms = query.negated_title_terms.clone();
    negated_title_terms.sort_unstable();
    let mut term_groups = query
        .term_groups
        .iter()
        .map(|expr| expr.canonical())
        .collect::<Vec<_>>();
    term_groups.sort_unstable();
    let mut negated_term_groups = query
        .negated_term_groups
        .iter()
        .map(|expr| expr.canonical())
        .collect::<Vec<_>>();
    negated_term_groups.sort_unstable();
    let payload = QueryHashPayload {
        tag_expr: query.tag_expr.as_ref().map(|expr| expr.canonical()),
        feed: query.feed.as_ref().map(|feed| match feed {
            FeedFilter::Id(id) => QueryHashFeed::Id(id.clone()),
            FeedFilter::Title(title) => QueryHashFeed::Title(title.clone()),
        }),
        title_terms,
        negated_title_terms,
        term_groups,
        negated_term_groups,
        after: query.after,
        before: query.before,
    };
    let payload = serde_json::to_vec(&payload).expect("serialize query hash payload");
    let mut hasher = Sha1::new();
    hasher.update(payload);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::compute_query_hash;
    use crate::query::EntryQuery;

    #[test]
    fn query_hash_distinguishes_tag_content_from_other_fields() {
        let embedded = EntryQuery::parse(Some(r#"tag:"x|feed_title=y""#), Some("unread"))
            .expect("query with quoted tag");
        let separate = EntryQuery::parse(Some(r#"tag:x feed:"y""#), Some("unread"))
            .expect("query with tag and feed title");

        assert_ne!(compute_query_hash(&embedded), compute_query_hash(&separate));
    }

    #[test]
    fn query_hash_is_stable_for_reordered_equivalent_terms() {
        let first =
            EntryQuery::parse(Some("alpha beta tag:A|B"), Some("unread")).expect("first query");
        let reordered =
            EntryQuery::parse(Some("beta alpha tag:B|A"), Some("unread")).expect("reordered query");

        assert_eq!(compute_query_hash(&first), compute_query_hash(&reordered));
    }
}
