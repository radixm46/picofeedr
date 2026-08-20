use super::EntryDetail;
use crate::config::AppConfig;
use crate::content_ref::sha256_path;
use crate::db::EntryContentStorage as Storage;
use crate::db::sqlite::{SqliteStore, repo::EntryReadRepo};
use crate::entry::EntryEnclosure;
use crate::error::{AppError, error_details};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: &str,
) -> Result<EntryDetail, AppError> {
    let entry_repo = store.entry_read_repo();
    let row = entry_repo.view_entry_row(entry_id)?.ok_or_else(|| {
        AppError::entry_not_found_with_details(
            format!("Entry {entry_id} not found"),
            error_details([
                ("resource", JsonValue::from("entry")),
                ("entry_id", JsonValue::from(entry_id.to_string())),
            ]),
        )
    })?;
    let tags = entry_repo
        .load_tags(&[row.entry_pk])?
        .remove(&row.entry_pk)
        .unwrap_or_default();
    let (content, content_type) =
        load_content(&entry_repo, &config.storage.data_dir, row.entry_pk)?;
    let enclosures = entry_repo
        .load_enclosure_rows(row.entry_pk)?
        .into_iter()
        .map(|row| EntryEnclosure {
            url: row.url,
            mime_type: row.mime_type,
            length: row.length,
        })
        .collect();

    Ok(EntryDetail {
        entry_id: row.entry_id,
        feed_id: row.feed_id,
        feed_title: row.feed_title,
        title: row.title,
        link: row.link,
        author: row.author,
        published_at: row.published_at,
        first_seen_at: row.first_seen_at,
        content,
        content_type,
        tags,
        enclosures,
    })
}

fn load_content(
    entry_repo: &EntryReadRepo<'_>,
    data_dir: &Path,
    entry_pk: i64,
) -> Result<(Option<String>, Option<String>), AppError> {
    let Some(row) = entry_repo.load_content_row(entry_pk)? else {
        return Ok((None, None));
    };
    match row.storage {
        Storage::Db => Ok((row.content, row.content_type)),
        Storage::Fs => {
            let reference = row
                .reference
                .ok_or_else(|| AppError::internal("Missing content reference for fs storage"))?;
            let path = sha256_path(data_dir, &reference)?;
            match fs::read_to_string(&path) {
                Ok(content) => Ok((Some(content), row.content_type)),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok((None, row.content_type)),
                Err(error) => Err(AppError::io_with_details_and_source(
                    "Failed to read entry content from filesystem",
                    error_details([
                        ("path", JsonValue::from(path.to_string_lossy().to_string())),
                        ("reference", JsonValue::from(reference)),
                        ("hint", JsonValue::from("failed_to_read_entry_content")),
                    ]),
                    error,
                )),
            }
        }
        Storage::None => Ok((None, row.content_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::load_content;
    use crate::db::sqlite::repo::EntryReadRepo;
    use rusqlite::Connection;
    use serde_json::Value as JsonValue;
    use std::fs;
    use tempfile::TempDir;

    fn create_entry_contents_table(conn: &Connection) {
        conn.execute(
            "CREATE TABLE entry_contents (
                entry_pk INTEGER PRIMARY KEY,
                storage TEXT NOT NULL,
                ref TEXT,
                content_type TEXT,
                content TEXT
            )",
            [],
        )
        .expect("create entry_contents table");
    }

    #[test]
    fn load_content_fs_not_found_returns_none_with_content_type() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        let reference = "a".repeat(64);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', ?1, 'text/html', NULL)",
            [&reference],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");

        let (content, content_type) =
            load_content(&EntryReadRepo::new(&conn), temp.path(), 1).expect("load content");

        assert_eq!(content, None);
        assert_eq!(content_type.as_deref(), Some("text/html"));
    }

    #[test]
    fn load_content_fs_reads_utf8_content_with_content_type() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        let reference = format!("ab{}", "c".repeat(62));
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', ?1, 'text/plain', NULL)",
            [&reference],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("ab").join(&reference);
        fs::create_dir_all(path.parent().expect("content parent directory"))
            .expect("create content directory");
        fs::write(&path, "本文です\n").expect("write content");

        let (content, content_type) =
            load_content(&EntryReadRepo::new(&conn), temp.path(), 1).expect("load content");

        assert_eq!(content.as_deref(), Some("本文です\n"));
        assert_eq!(content_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn load_content_fs_read_error_returns_io_error() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        let reference = "a".repeat(64);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', ?1, 'text/html', NULL)",
            [&reference],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("aa").join(&reference);
        fs::create_dir_all(&path).expect("create directory at content path");

        let error = load_content(&EntryReadRepo::new(&conn), temp.path(), 1)
            .expect_err("directory read should fail");

        assert_eq!(error.code().as_str(), "IO_ERROR");
        let details = error.details().expect("details");
        assert_eq!(details["hint"], "failed_to_read_entry_content");
        assert!(
            details
                .get("reference")
                .and_then(JsonValue::as_str)
                .is_some_and(|value| !value.is_empty())
        );
        assert!(
            details
                .get("path")
                .and_then(JsonValue::as_str)
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[test]
    fn load_content_fs_missing_reference_returns_internal_error() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', NULL, NULL, NULL)",
            [],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");

        let error = load_content(&EntryReadRepo::new(&conn), temp.path(), 1)
            .expect_err("missing reference should fail");

        assert_eq!(error.code().as_str(), "INTERNAL");
        assert_eq!(error.message(), "Missing content reference for fs storage");
    }

    #[test]
    fn load_content_fs_invalid_reference_returns_internal_error() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', 'not-a-sha256', NULL, NULL)",
            [],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");

        let error = load_content(&EntryReadRepo::new(&conn), temp.path(), 1)
            .expect_err("invalid reference should fail");

        assert_eq!(error.code().as_str(), "INTERNAL");
        assert_eq!(error.message(), "Invalid content reference length: 12");
    }
}
