//! The bounded stdin of `raptor-mcp` (ADR-MCP-001 § 6, NFR-02, SEC-02): one
//! JSON-RPC message per line, at most [`MAX_MESSAGE_BYTES`] and nested at most
//! [`MAX_DEPTH`] deep, the same limits as the channel. The check runs before
//! rmcp parses anything, so an oversized or endlessly nested message never
//! reaches serde.
//!
//! A refused message gets one JSON-RPC error with `id: null` (the message was
//! not parsed, so its id is unknown), `code` [`code::INVALID_REQUEST`] and
//! `data.reason` of `message-too-large` or `message-too-deep`. The server
//! stays alive: an oversized line is drained up to its `\n` without being
//! kept, and the next line is served as usual.

use std::io;

use gitraptor_api::framing::{MAX_DEPTH, MAX_MESSAGE_BYTES, depth_within};
use gitraptor_api::rpc::code;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

/// Why a message was refused. `reason` is the stable `data.reason` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooLarge,
    TooDeep,
}

impl Refusal {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::TooLarge => "message-too-large",
            Self::TooDeep => "message-too-deep",
        }
    }

    /// The JSON-RPC error line (without the newline).
    pub fn response(self) -> Vec<u8> {
        let message = match self {
            Self::TooLarge => "message too large",
            Self::TooDeep => "message too deep",
        };
        let body: Value = json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {
                "code": code::INVALID_REQUEST,
                "message": message,
                "data": {"reason": self.reason()},
            },
        });
        body.to_string().into_bytes()
    }
}

/// One line read from stdin.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// A line within the limits, left in the caller's buffer.
    Message,
    /// A refused line; the stream is already at the start of the next one.
    Refused(Refusal),
    /// End of stream.
    End,
}

/// Reads one line into `buf` (cleared first), without the newline. Never keeps
/// more than `max + 1` bytes of one line: past `max` the rest is drained and
/// dropped. Depth is checked on the whole line before it is handed over.
pub async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> io::Result<Frame> {
    buf.clear();
    let mut too_large = false;
    loop {
        let available = match reader.fill_buf().await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if available.is_empty() {
            return Ok(if too_large {
                Frame::Refused(Refusal::TooLarge)
            } else if buf.is_empty() {
                Frame::End
            } else {
                checked(buf)
            });
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
            return Ok(if too_large {
                Frame::Refused(Refusal::TooLarge)
            } else {
                checked(buf)
            });
        }
    }
}

fn checked(buf: &mut Vec<u8>) -> Frame {
    if depth_within(buf, MAX_DEPTH).is_none() {
        buf.clear();
        return Frame::Refused(Refusal::TooDeep);
    }
    Frame::Message
}

/// Writes one whole line under the lock, so the server's replies and the
/// refusals never interleave on stdout.
pub async fn write_line<W: AsyncWrite + Unpin>(out: &Mutex<W>, line: &[u8]) -> io::Result<()> {
    let mut out = out.lock().await;
    out.write_all(line).await?;
    out.write_all(b"\n").await?;
    out.flush().await
}

/// Pumps `input` into `server` line by line under the limits, answering a
/// refused line on `out`. Returns at end of input, so dropping `server`
/// closes rmcp's stdin.
pub async fn pump_input<R, S, W>(input: R, mut server: S, out: &Mutex<W>) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    S: AsyncWrite + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input = input;
    let mut buf = Vec::new();
    loop {
        match read_frame(&mut input, &mut buf, MAX_MESSAGE_BYTES).await? {
            Frame::End => return Ok(()),
            Frame::Refused(refusal) => write_line(out, &refusal.response()).await?,
            Frame::Message => {
                buf.push(b'\n');
                server.write_all(&buf).await?;
                server.flush().await?;
            }
        }
    }
}

/// Copies the server's output to `out` line by line (rmcp writes one message
/// per line), until the server closes it.
pub async fn pump_output<R, W>(server: R, out: &Mutex<W>) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut server = server;
    let mut line = Vec::new();
    loop {
        line.clear();
        if server.read_until(b'\n', &mut line).await? == 0 {
            return Ok(());
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        write_line(out, &line).await?;
    }
}

#[cfg(test)]
mod tests;
