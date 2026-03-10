//! Entry view/mark operations and response types.

mod list;

use crate::config::AppConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::{AppError, error_details};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashSet;

pub use list::list_entries;

/// Entry summary for list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntrySummary {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
}

/// Entry enclosure payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryEnclosure {
    /// Enclosure URL.
    pub url: String,
    /// MIME type.
    pub mime_type: Option<String>,
    /// Length in bytes.
    pub length: Option<i64>,
}

/// Entry detail payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryDetail {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
    /// Feed title.
    pub feed_title: Option<String>,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Entry author.
    pub author: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Entry content body.
    pub content: Option<String>,
    /// Content type.
    pub content_type: Option<String>,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
    /// Enclosure list.
    pub enclosures: Vec<EntryEnclosure>,
}

/// Entry list response payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryListResponse {
    /// Total hits for the query.
    pub total_count: i64,
    /// Page items.
    pub items: Vec<EntrySummary>,
    /// Feed dictionary for feed id to title mapping.
    pub feeds: Vec<FeedSummary>,
    /// Cursor for the next page.
    pub next_page_token: Option<String>,
    /// Revision captured when the list was fetched.
    pub revision: i64,
    /// Write timestamp captured when the list was fetched.
    pub last_write_at: Option<i64>,
}

/// Feed dictionary item used by entry list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedSummary {
    /// Feed id.
    pub feed_id: String,
    /// Feed title.
    pub title: Option<String>,
}

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: &str,
) -> Result<EntryDetail, AppError> {
    let entry_repo = store.entry_read_repo();
    let row = entry_repo.view_entry_row(entry_id)?;
    let (entry_pk, entry_id, feed_id, feed_title, title, link, author, published_at, first_seen_at) =
        row.ok_or_else(|| {
            AppError::entry_not_found_with_details(
                format!("Entry {entry_id} not found"),
                error_details([
                    ("resource", JsonValue::from("entry")),
                    ("entry_id", JsonValue::from(entry_id.to_string())),
                ]),
            )
        })?;
    let tags = entry_repo
        .load_tags(&[entry_pk])?
        .remove(&entry_pk)
        .unwrap_or_default();
    let (content, content_type) = entry_repo.load_content(&config.storage.data_dir, entry_pk)?;
    let enclosures = entry_repo.load_enclosures(entry_pk)?;

    Ok(EntryDetail {
        entry_id,
        feed_id,
        feed_title,
        title,
        link,
        author,
        published_at,
        first_seen_at,
        content,
        content_type,
        tags,
        enclosures,
    })
}

/// Updates entry tags and returns the number of affected entries.
///
/// Returns `ENTRY_NOT_FOUND` when any requested entry id does not exist.
pub fn mark_entries(
    store: &mut SqliteStore,
    entry_ids: &[String],
    add_tags: &[String],
    remove_tags: &[String],
) -> Result<usize, AppError> {
    if add_tags.is_empty() && remove_tags.is_empty() {
        return Err(AppError::invalid_query(
            "mark tag requires --add or --remove",
        ));
    }
    let mut unique_ids = Vec::new();
    let mut seen = HashSet::new();
    for id in entry_ids {
        if seen.insert(id.clone()) {
            unique_ids.push(id.clone());
        }
    }
    if unique_ids.is_empty() {
        return Ok(0);
    }
    let tx = store.tx()?;
    let tx_entry_repo = tx.entry_write_repo();
    tx_entry_repo.ensure_all_entry_ids_exist(&unique_ids)?;
    let entry_pks = tx_entry_repo.find_entry_pks_by_ids(&unique_ids)?;
    let add_ids = tx_entry_repo.ensure_tag_ids(add_tags)?;
    let remove_ids = tx_entry_repo.lookup_tag_ids(remove_tags)?;
    let mut updated = 0usize;
    for entry_id in unique_ids {
        let Some(entry_pk) = entry_pks.get(&entry_id).copied() else {
            continue;
        };
        let mut changed = false;
        for tag_id in add_ids.values() {
            let rows = tx_entry_repo.insert_entry_tag(entry_pk, *tag_id)?;
            if rows > 0 {
                changed = true;
            }
        }
        for tag_id in remove_ids.values() {
            let rows = tx_entry_repo.delete_entry_tag(entry_pk, *tag_id)?;
            if rows > 0 {
                changed = true;
            }
        }
        if changed {
            updated += 1;
        }
    }
    tx.commit()?;
    Ok(updated)
}
