//! Unit tests for RbdSnapName parsing, display, and serde.

use cephalopod::RbdSnapName;

#[test]
fn test_parse_valid() {
    let snap: RbdSnapName = "my-image@snap1".parse().unwrap();
    assert_eq!(snap.image_name, "my-image");
    assert_eq!(snap.snap_name, "snap1");
}

#[test]
fn test_parse_with_namespace() {
    let snap: RbdSnapName = "owner_id/my-image@snap1".parse().unwrap();
    assert_eq!(snap.image_name, "owner_id/my-image");
    assert_eq!(snap.snap_name, "snap1");
}

#[test]
fn test_parse_no_at_sign() {
    let result: Result<RbdSnapName, _> = "no-at-sign".parse();
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_image() {
    let result: Result<RbdSnapName, _> = "@snap1".parse();
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_snap() {
    let result: Result<RbdSnapName, _> = "image@".parse();
    assert!(result.is_err());
}

#[test]
fn test_parse_multiple_at_signs() {
    // Only first @ should be the split point — but our impl uses split_once
    // so "a@b@c" → image="a", snap="b@c"
    let result: Result<RbdSnapName, _> = "a@b@c".parse();
    // This should succeed — snap names could theoretically contain @
    assert!(result.is_ok());
    let snap = result.unwrap();
    assert_eq!(snap.image_name, "a");
    assert_eq!(snap.snap_name, "b@c");
}

#[test]
fn test_display() {
    let snap = RbdSnapName {
        image_name: "my-image".to_string(),
        snap_name: "snap1".to_string(),
    };
    assert_eq!(snap.to_string(), "my-image@snap1");
}

#[test]
fn test_display_with_namespace() {
    let snap = RbdSnapName {
        image_name: "owner/my-image".to_string(),
        snap_name: "snap1".to_string(),
    };
    assert_eq!(snap.to_string(), "owner/my-image@snap1");
}

#[test]
fn test_roundtrip_parse_display() {
    let original = "ns/image@snapname";
    let snap: RbdSnapName = original.parse().unwrap();
    assert_eq!(snap.to_string(), original);
}

#[test]
fn test_serde_json_roundtrip() {
    let snap = RbdSnapName {
        image_name: "my-image".to_string(),
        snap_name: "snap1".to_string(),
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(json, r#""my-image@snap1""#);

    let deserialized: RbdSnapName = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.image_name, "my-image");
    assert_eq!(deserialized.snap_name, "snap1");
}

#[test]
fn test_serde_deserialize_invalid() {
    let result: Result<RbdSnapName, _> = serde_json::from_str(r#""no-at-sign""#);
    assert!(result.is_err());
}

#[test]
fn test_serde_deserialize_with_namespace() {
    let snap: RbdSnapName = serde_json::from_str(r#""owner/image@snap""#).unwrap();
    assert_eq!(snap.image_name, "owner/image");
    assert_eq!(snap.snap_name, "snap");
}
