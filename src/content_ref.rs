//! Content reference helpers for filesystem storage.

use crate::error::AppError;
use std::path::{Path, PathBuf};

/// Builds the filesystem path for a sha256 hex content reference.
pub(crate) fn sha256_path(root: &Path, reference: &str) -> Result<PathBuf, AppError> {
    validate_sha256(reference)?;
    Ok(root.join(&reference[0..2]).join(reference))
}

/// Validates that the reference is a lowercase sha256 hex digest.
pub(crate) fn validate_sha256(reference: &str) -> Result<(), AppError> {
    if reference.len() != 64 {
        return Err(AppError::internal(format!(
            "Invalid content reference length: {}",
            reference.len()
        )));
    }
    if !reference
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(AppError::internal(
            "Invalid content reference characters".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_sha256_accepts_lowercase_hex() {
        let reference = "a".repeat(64);
        validate_sha256(&reference).expect("valid ref");
    }

    #[test]
    fn validate_sha256_rejects_short_refs() {
        let error = validate_sha256("abc").expect_err("invalid");
        assert!(
            error
                .to_string()
                .contains("Invalid content reference length")
        );
    }

    #[test]
    fn validate_sha256_rejects_non_hex() {
        let mut reference = "a".repeat(64);
        reference.replace_range(0..1, "g");
        assert!(validate_sha256(&reference).is_err());
    }

    #[test]
    fn sha256_path_uses_two_char_prefix() {
        let reference = format!("ab{}", "c".repeat(62));
        let root = Path::new("/tmp");
        let path = sha256_path(root, &reference).expect("path");
        assert!(
            path.to_string_lossy()
                .ends_with(&format!("/ab/{reference}"))
        );
    }
}
