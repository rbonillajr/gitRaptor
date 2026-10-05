//! Strict parsing of the hook input (ADR-GRD-002 § 4, SEC-GRD-07): every
//! line is validated (shape, object id length, `check-ref-format`) and bounded.
//! A line that fails is never taken through the fast path: the caller treats
//! the whole transaction as governed and fails closed.

use gitraptor_api::guard::RefValue;
use gitraptor_git::RefName;

/// Longest accepted input line, in bytes. Only lines are bounded, not the
/// number of lines (a `fetch` of thousands of refs is one transaction).
pub const MAX_LINE_BYTES: usize = 8 * 1024;

/// Why a line was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineError {
    TooLong,
    NotUtf8,
    Shape,
    ObjectId,
    RefName,
}

/// One `reference-transaction` line as Git wrote it: `<old> <new> <ref>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRefUpdate {
    pub old: RefValue,
    pub new: RefValue,
    pub refname: String,
}

/// One `pre-push` line: `<local ref> <local oid> <remote ref> <remote oid>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPushUpdate {
    /// `None` for `(delete)`.
    pub local_ref: Option<String>,
    pub local: RefValue,
    pub remote_ref: String,
    pub remote: RefValue,
}

fn text(line: &[u8]) -> Result<&str, LineError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(LineError::TooLong);
    }
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    std::str::from_utf8(line).map_err(|_| LineError::NotUtf8)
}

/// An object id: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits, all zero
/// meaning "absent".
fn object_id(field: &str) -> Result<RefValue, LineError> {
    let hex = field
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex || !(field.len() == 40 || field.len() == 64) {
        return Err(LineError::ObjectId);
    }
    Ok(if field.bytes().all(|b| b == b'0') {
        RefValue::Zero
    } else {
        RefValue::Oid(field.to_owned())
    })
}

/// A value of `reference-transaction`: an object id or `ref:<target>`
/// (symbolic refs, Git 2.4x onwards; E-02-3).
fn ref_value(field: &str) -> Result<RefValue, LineError> {
    match field.strip_prefix("ref:") {
        Some(target) => Ok(RefValue::Symbolic(ref_name(target)?)),
        None => object_id(field),
    }
}

fn ref_name(name: &str) -> Result<String, LineError> {
    RefName::new(name)
        .map(|r| r.as_str().to_owned())
        .map_err(|_| LineError::RefName)
}

/// Parses one `reference-transaction` line.
pub fn parse_ref_update(line: &[u8]) -> Result<RawRefUpdate, LineError> {
    let line = text(line)?;
    let mut fields = line.split(' ');
    let (Some(old), Some(new), Some(refname), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(LineError::Shape);
    };
    Ok(RawRefUpdate {
        old: ref_value(old)?,
        new: ref_value(new)?,
        refname: ref_name(refname)?,
    })
}

/// Parses one `pre-push` line.
pub fn parse_push_update(line: &[u8]) -> Result<RawPushUpdate, LineError> {
    let line = text(line)?;
    let mut fields = line.split(' ');
    let (Some(local_ref), Some(local), Some(remote_ref), Some(remote), None) = (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) else {
        return Err(LineError::Shape);
    };
    let local = object_id(local)?;
    let local_ref = match local_ref {
        "(delete)" if local.is_zero() => None,
        "(delete)" => return Err(LineError::Shape),
        // Any local spelling Git resolved (a ref, `HEAD`, an object id):
        // only shown in messages, never used to decide.
        r if !r.is_empty() && r.chars().all(|c| !c.is_control()) => Some(r.to_owned()),
        _ => return Err(LineError::Shape),
    };
    Ok(RawPushUpdate {
        local_ref,
        local,
        remote_ref: ref_name(remote_ref)?,
        remote: object_id(remote)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const Z: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn reference_transaction_lines() {
        let u = parse_ref_update(format!("{A} {Z} refs/heads/main\n").as_bytes()).unwrap();
        assert_eq!(u.old, RefValue::Oid(A.into()));
        assert!(u.new.is_zero());
        assert_eq!(u.refname, "refs/heads/main");

        let sym = parse_ref_update(format!("{Z} ref:refs/heads/feat HEAD").as_bytes()).unwrap();
        assert_eq!(sym.new, RefValue::Symbolic("refs/heads/feat".into()));

        let sha256 = "a".repeat(64);
        assert!(parse_ref_update(format!("{sha256} {Z} refs/heads/x").as_bytes()).is_ok());
    }

    #[test]
    fn malformed_lines_are_rejected() {
        for bad in [
            format!("{A} {Z}"),
            format!("{A} {Z} refs/heads/a b"),
            format!("{A}0 {Z} refs/heads/x"),
            format!("{} {Z} refs/heads/x", "ABCDEF".repeat(7).split_at(40).0),
            format!("{A} {Z} refs/heads/x..y"),
            format!("{A} {Z} -refs"),
            format!("{A} ref: refs/heads/x"),
        ] {
            assert!(parse_ref_update(bad.as_bytes()).is_err(), "{bad}");
        }
        let long = format!("{A} {Z} refs/heads/{}", "x".repeat(MAX_LINE_BYTES));
        assert_eq!(parse_ref_update(long.as_bytes()), Err(LineError::TooLong));
        assert_eq!(parse_ref_update(b"\xff\xfe"), Err(LineError::NotUtf8));
    }

    #[test]
    fn pre_push_lines() {
        let del =
            parse_push_update(format!("(delete) {Z} refs/heads/main {A}").as_bytes()).unwrap();
        assert_eq!(del.local_ref, None);
        assert!(del.local.is_zero());
        let upd =
            parse_push_update(format!("refs/heads/f {A} refs/heads/f {Z}\n").as_bytes()).unwrap();
        assert_eq!(upd.local_ref.as_deref(), Some("refs/heads/f"));
        assert!(parse_push_update(format!("(delete) {A} refs/heads/main {A}").as_bytes()).is_err());
        assert!(parse_push_update(format!("x {A} ref:refs/heads/x {A}").as_bytes()).is_err());
    }
}
