//! Message framing and bounded decoding (SEC-02).
//!
//! One JSON message per line (`\n`). A line longer than
//! [`MAX_MESSAGE_BYTES`] or nested deeper than [`MAX_DEPTH`] is refused
//! before serde sees it; a batch (a top-level array) is refused too. These
//! functions never panic on any input (see the property test).

use std::io::{self, BufRead};

use crate::rpc::{ErrorObject, Request, code};

/// Largest accepted message, without the newline.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Deepest accepted nesting of arrays and objects.
pub const MAX_DEPTH: usize = 32;

/// Why a frame could not be read.
#[derive(Debug)]
pub enum FrameError {
    /// The line exceeded the size limit. The rest of the line was consumed.
    TooLarge,
    Io(io::Error),
}

/// Reads one line into `buf` (cleared first), without the trailing newline.
/// Returns `Ok(false)` at end of stream. Never buffers more than
/// `max + 1` bytes of one line: an oversized line is drained and refused.
pub fn read_frame<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> Result<bool, FrameError> {
    buf.clear();
    let mut too_large = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(FrameError::Io(err)),
        };
        if available.is_empty() {
            if too_large {
                return Err(FrameError::TooLarge);
            }
            return Ok(!buf.is_empty());
        }
        let (chunk, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (&available[..i], Some(i + 1)),
            None => (available, None),
        };
        if !too_large {
            if buf.len() + chunk.len() > max {
                too_large = true;
                buf.clear();
            } else {
                buf.extend_from_slice(chunk);
            }
        }
        let consumed = done.unwrap_or(available.len());
        reader.consume(consumed);
        if done.is_some() {
            return if too_large {
                Err(FrameError::TooLarge)
            } else {
                Ok(true)
            };
        }
    }
}

/// Nesting depth of a JSON text, counted outside strings. Returns `None` as
/// soon as it exceeds `max`.
pub fn depth_within(bytes: &[u8], max: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut deepest = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &b in bytes {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
                if depth > max {
                    return None;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Some(deepest)
}

/// Decodes one client message, refusing batches, excessive depth, unknown
/// fields and a wrong `jsonrpc` value.
pub fn decode_request(bytes: &[u8]) -> Result<Request, ErrorObject> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ErrorObject::new(code::INVALID_REQUEST, "message too large"));
    }
    let first = bytes.iter().find(|b| !b.is_ascii_whitespace());
    if first == Some(&b'[') {
        return Err(ErrorObject::new(
            code::INVALID_REQUEST,
            "batches are not supported",
        ));
    }
    if depth_within(bytes, MAX_DEPTH).is_none() {
        return Err(ErrorObject::new(code::INVALID_REQUEST, "message too deep"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| ErrorObject::new(code::PARSE_ERROR, "parse error"))?;
    if !value.is_object() {
        return Err(ErrorObject::new(code::INVALID_REQUEST, "not a request"));
    }
    let request: Request = serde_json::from_value(value).map_err(|err| {
        ErrorObject::new(code::INVALID_REQUEST, &format!("invalid request: {err}"))
    })?;
    if request.jsonrpc != crate::rpc::JSONRPC {
        return Err(ErrorObject::new(
            code::INVALID_REQUEST,
            "jsonrpc must be 2.0",
        ));
    }
    if !request.id.is_valid() {
        return Err(ErrorObject::new(code::INVALID_REQUEST, "id too long"));
    }
    if request.method.is_empty() || request.method.len() > 64 {
        return Err(ErrorObject::new(code::INVALID_REQUEST, "invalid method"));
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lines_and_refuses_an_oversized_one() {
        let big = "x".repeat(20);
        let input = format!("{{}}\n{big}\n[]\n");
        let mut reader = io::BufReader::with_capacity(4, input.as_bytes());
        let mut buf = Vec::new();
        assert!(read_frame(&mut reader, &mut buf, 10).unwrap());
        assert_eq!(buf, b"{}");
        assert!(matches!(
            read_frame(&mut reader, &mut buf, 10),
            Err(FrameError::TooLarge)
        ));
        // The stream stays in sync after the refused line.
        assert!(read_frame(&mut reader, &mut buf, 10).unwrap());
        assert_eq!(buf, b"[]");
        assert!(!read_frame(&mut reader, &mut buf, 10).unwrap());
    }

    #[test]
    fn refuses_batches_depth_and_bad_versions() {
        let batch = br#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#;
        assert_eq!(
            decode_request(batch).unwrap_err().code,
            code::INVALID_REQUEST
        );
        let deep = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"x","params":{}{}}}"#,
            "[".repeat(40),
            "]".repeat(40)
        );
        assert_eq!(
            decode_request(deep.as_bytes()).unwrap_err().message,
            "message too deep"
        );
        let v1 = br#"{"jsonrpc":"1.0","id":1,"method":"x"}"#;
        assert!(decode_request(v1).is_err());
        let brackets_in_string = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"x","params":{{"s":"{}"}}}}"#,
            "[".repeat(100)
        );
        assert!(decode_request(brackets_in_string.as_bytes()).is_ok());
    }

    /// Property test of the decoder (stand-in for `cargo-fuzz`, which needs
    /// nightly): random and mutated inputs never panic and never decode into
    /// something that breaks the limits.
    #[test]
    fn decoder_never_panics_on_random_input() {
        let seeds: &[&[u8]] = &[
            br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":1}}"#,
            br#"{"jsonrpc":"2.0","id":"x","method":"events.subscribe","params":{"from_seq":3}}"#,
            b"[[[[{\"a\":\"\\\"\"}]]]]",
            b"",
            b"\xff\xfe\x00",
        ];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = b"{}[]\":,\\0123456789abcdefnulltrue -.eE\x00\xc3\xa9";
        for round in 0..20_000 {
            let mut input = seeds[round % seeds.len()].to_vec();
            let edits = (next() % 8) as usize;
            for _ in 0..edits {
                let r = next();
                let byte = alphabet[(r % alphabet.len() as u64) as usize];
                match (r >> 8) % 3 {
                    0 if !input.is_empty() => {
                        let at = (r >> 16) as usize % input.len();
                        input[at] = byte;
                    }
                    1 => {
                        let at = (r >> 16) as usize % (input.len() + 1);
                        input.insert(at, byte);
                    }
                    _ if !input.is_empty() => {
                        let at = (r >> 16) as usize % input.len();
                        input.remove(at);
                    }
                    _ => {}
                }
            }
            if let Ok(request) = decode_request(&input) {
                assert!(request.id.is_valid());
                assert_eq!(request.jsonrpc, "2.0");
            }
            let mut reader = io::BufReader::new(input.as_slice());
            let mut buf = Vec::new();
            while let Ok(true) = read_frame(&mut reader, &mut buf, 64) {
                assert!(buf.len() <= 64);
            }
        }
    }
}
