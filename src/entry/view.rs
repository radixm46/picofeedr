use super::EntryDetail;
use crate::config::AppConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::{AppError, error_details};
use serde_json::Value as JsonValue;

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
