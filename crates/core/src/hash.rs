//! Content hashes for library data: `sha256:` over canonical JSON (object keys sorted, no
//! whitespace), so the same symbol, footprint or component always hashes the same regardless of
//! field order or formatting.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Canonical JSON text of `value`.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                if let Some(v) = map.get(k) {
                    write_canonical(v, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `sha256:<hex>` of raw bytes.
pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256:");
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// `sha256:<hex>` of a JSON value's canonical form.
pub fn value_hash(value: &Value) -> String {
    sha256(canonical_json(value).as_bytes())
}

/// `sha256:<hex>` of any serializable value's canonical JSON. Library types always serialize,
/// so a failure (which would mean a non-string map key) hashes as `null`.
pub fn content_hash<T: Serialize>(value: &T) -> String {
    value_hash(&serde_json::to_value(value).unwrap_or(Value::Null))
}

/// The hex digits of a `sha256:` hash, for file names.
pub fn hex_of(hash: &str) -> &str {
    hash.strip_prefix("sha256:").unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_hash() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":[1,2],"x":"s"}}"#).unwrap();
        let b = json!({"a": {"x": "s", "y": [1, 2]}, "b": 1});
        assert_eq!(canonical_json(&a), r#"{"a":{"x":"s","y":[1,2]},"b":1}"#);
        assert_eq!(value_hash(&a), value_hash(&b));
        assert_ne!(value_hash(&a), value_hash(&json!({"b": 2})));
    }

    #[test]
    fn hashes_are_prefixed_hex() {
        let h = sha256(b"abc");
        assert_eq!(h, "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex_of(&h).len(), 64);
    }
}
