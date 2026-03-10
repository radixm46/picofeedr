use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use std::collections::HashSet;

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
