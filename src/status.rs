//! Status response payload helpers.

use crate::db::sqlite::SystemMeta;
use serde::Serialize;

/// Status payload for lightweight database polling.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    /// Monotonic revision incremented after successful write commands.
    pub revision: i64,
    /// Epoch seconds for the latest successful write command.
    pub updated_at: Option<i64>,
    /// Current database schema version.
    pub schema_version: i64,
    /// Current CLI API version.
    pub api_version: &'static str,
    /// Epoch seconds for the latest successful sync command.
    pub sync_at: Option<i64>,
    /// Latest successful sync status.
    pub sync_status: Option<String>,
}

impl StatusResponse {
    /// Builds status payload from persisted metadata and build-time versions.
    pub fn from_meta(meta: &SystemMeta, schema_version: i64, api_version: &'static str) -> Self {
        Self {
            revision: meta.revision,
            updated_at: meta.updated_at,
            schema_version,
            api_version,
            sync_at: meta.sync_at,
            sync_status: meta.sync_status.clone(),
        }
    }
}
