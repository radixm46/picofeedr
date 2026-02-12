//! Assertion helpers for CLI integration tests.

use serde_json::Value;

/// Extracts the `data` object from a successful JSON envelope.
pub fn extract_ok_data(output: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], true, "expected ok=true envelope");
    value.get("data").cloned().expect("data")
}

/// Extracts error code from a failed JSON envelope.
pub fn extract_error_code(output: &[u8]) -> String {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], false, "expected ok=false envelope");
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(|code| code.as_str())
        .expect("error code")
        .to_string()
}

/// Extracts error payload from a failed JSON envelope.
pub fn extract_error_payload(output: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], false, "expected ok=false envelope");
    value.get("error").cloned().expect("error")
}

/// Asserts a failed JSON envelope with expected error metadata.
pub fn assert_error_envelope(output: &[u8], code: &str, retry: bool) {
    let error = extract_error_payload(output);
    assert_eq!(error["code"], code);
    assert_eq!(error["retry"], retry);
}

/// Asserts plain output contract: non-JSON and required snippets are present.
pub fn assert_plain_contract(output: &[u8], required_snippets: &[&str]) {
    let parsed = serde_json::from_slice::<Value>(output);
    assert!(parsed.is_err(), "expected plain (non-JSON) output");

    let output_str = String::from_utf8_lossy(output);
    let line_count = output_str.lines().count();
    assert!(line_count >= 2, "expected multi-line plain output");
    for snippet in required_snippets {
        assert!(
            output_str.contains(snippet),
            "expected plain output to contain `{snippet}`"
        );
    }
}
