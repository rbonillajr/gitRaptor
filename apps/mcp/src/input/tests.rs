use gitraptor_api::framing::{MAX_DEPTH, MAX_MESSAGE_BYTES};
use tokio::io::{AsyncReadExt, BufReader};

use super::*;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

/// A JSON-RPC-looking message of exactly `len` bytes.
fn message_of(len: usize) -> Vec<u8> {
    let head = br#"{"a":""#;
    let tail = br#""}"#;
    let mut m = head.to_vec();
    m.resize(len - tail.len(), b'x');
    m.extend_from_slice(tail);
    assert_eq!(m.len(), len);
    m
}

fn nested(depth: usize) -> Vec<u8> {
    let mut m = vec![b'['; depth];
    m.extend(std::iter::repeat_n(b']', depth));
    m
}

async fn one(line: &[u8]) -> Frame {
    let mut reader = BufReader::new(line);
    let mut buf = Vec::new();
    read_frame(&mut reader, &mut buf, MAX_MESSAGE_BYTES)
        .await
        .unwrap()
}

#[test]
fn a_message_of_exactly_one_mib_is_served_and_one_byte_more_is_refused() {
    block_on(async {
        assert_eq!(one(&message_of(MAX_MESSAGE_BYTES)).await, Frame::Message);
        assert_eq!(
            one(&message_of(MAX_MESSAGE_BYTES + 1)).await,
            Frame::Refused(Refusal::TooLarge)
        );
    });
}

#[test]
fn depth_32_is_served_and_depth_33_is_refused() {
    block_on(async {
        assert_eq!(one(&nested(MAX_DEPTH)).await, Frame::Message);
        assert_eq!(
            one(&nested(MAX_DEPTH + 1)).await,
            Frame::Refused(Refusal::TooDeep)
        );
        assert_eq!(one(&nested(200)).await, Frame::Refused(Refusal::TooDeep));
    });
}

#[test]
fn a_refused_message_is_answered_and_the_next_one_is_served() {
    block_on(async {
        for bad in [message_of(MAX_MESSAGE_BYTES + 1), nested(MAX_DEPTH + 1)] {
            let mut input = bad.clone();
            input.extend_from_slice(b"\n{\"ok\":1}\n");
            let out = Mutex::new(Vec::new());
            let mut server = Vec::new();
            pump_input(BufReader::new(input.as_slice()), &mut server, &out)
                .await
                .unwrap();
            assert_eq!(server, b"{\"ok\":1}\n");
            let out = out.into_inner();
            let reply: Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(reply["jsonrpc"], "2.0");
            assert_eq!(reply["id"], Value::Null);
            assert_eq!(reply["error"]["code"], code::INVALID_REQUEST);
            let reason = reply["error"]["data"]["reason"].as_str().unwrap();
            assert!(["message-too-large", "message-too-deep"].contains(&reason));
            assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 1);
        }
    });
}

#[test]
fn a_fifty_mib_message_is_dropped_without_keeping_it() {
    block_on(async {
        const HUGE: u64 = 50 * 1024 * 1024;
        let stream = tokio::io::repeat(b'x')
            .take(HUGE)
            .chain(&b"\n{\"ok\":1}\n"[..]);
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        let first = read_frame(&mut reader, &mut buf, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        assert_eq!(first, Frame::Refused(Refusal::TooLarge));
        assert!(buf.capacity() <= MAX_MESSAGE_BYTES, "{}", buf.capacity());
        let next = read_frame(&mut reader, &mut buf, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        assert_eq!((next, buf.as_slice()), (Frame::Message, &b"{\"ok\":1}"[..]));
    });
}

#[test]
fn a_last_line_without_newline_is_served_and_blank_end_is_end() {
    block_on(async {
        assert_eq!(one(b"{}").await, Frame::Message);
        assert_eq!(one(b"").await, Frame::End);
    });
}
