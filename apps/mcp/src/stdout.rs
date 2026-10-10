//! A stdout whose writes never wait: they join a queue that a task of their own empties.
//!
//! rmcp answers a message of the wrong shape (`-32600`) from inside the very future that reads
//! the next message, and its service loop drops that future whenever another branch of its
//! `select!` is ready first. With a writer that can be pending (tokio's stdout hands every write
//! to a blocking thread, slow on Windows under load) the answer was sometimes dropped half way
//! and the client heard nothing (#233). A write that is always ready cannot be cut half way.
//!
//! Order is kept (one queue, one task) and the queue is bounded in bytes: for a client that stops
//! reading, writes fail past the bound (rmcp then drops those answers and keeps serving) instead
//! of this process growing without limit (NFR-02).

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

/// Most bytes waiting to be written. A response is capped far below this (`MCP_REFUSAL_TOKENS`,
/// `MAX_MCP_PART_BYTES`), so only a client that does not read can reach it.
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;

/// The write half given to the transport.
pub struct QueuedWriter {
    queue: UnboundedSender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
}

/// What is left to write once the transport is gone.
pub struct Drain {
    task: JoinHandle<()>,
}

/// Queues the writes for `inner`, which a task of its own empties.
///
/// Must be called inside a Tokio runtime.
pub fn queued<W>(mut inner: W) -> (QueuedWriter, Drain)
where
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (queue, mut rx) = unbounded_channel::<Vec<u8>>();
    let queued = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&queued);
    let task = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let len = chunk.len();
            let written = inner.write_all(&chunk).await;
            counter.fetch_sub(len, Ordering::Relaxed);
            if written.is_err() || inner.flush().await.is_err() {
                // The reader is gone: nothing more can be said. Writes that follow find the
                // channel closed.
                return;
            }
        }
        let _ = inner.flush().await;
    });
    (QueuedWriter { queue, queued }, Drain { task })
}

impl Drain {
    /// Waits, at most `limit`, for what is queued to be written. Call it after the transport
    /// (and so the [`QueuedWriter`]) is dropped: the task ends when its queue is empty and closed.
    pub async fn finish(self, limit: Duration) {
        let _ = tokio::time::timeout(limit, self.task).await;
    }
}

impl AsyncWrite for QueuedWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let writer = self.get_mut();
        if writer
            .queued
            .load(Ordering::Relaxed)
            .saturating_add(buf.len())
            > MAX_QUEUED_BYTES
        {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the client does not read",
            )));
        }
        writer.queued.fetch_add(buf.len(), Ordering::Relaxed);
        if writer.queue.send(buf.to_vec()).is_err() {
            writer.queued.fetch_sub(buf.len(), Ordering::Relaxed);
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        Poll::Ready(Ok(buf.len()))
    }

    /// Queued is as good as written for the caller: [`Drain::finish`] waits for the rest.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn writes_arrive_whole_and_in_order_after_the_writer_is_dropped() {
        run(async {
            let (sink, mut far) = tokio::io::duplex(64);
            let (mut writer, drain) = queued(sink);
            for n in 0..50u8 {
                // More than the pipe holds: the writes still never wait.
                writer.write_all(&[n; 40]).await.unwrap();
                writer.flush().await.unwrap();
            }
            drop(writer);
            let reader = tokio::spawn(async move {
                let mut all = Vec::new();
                far.read_to_end(&mut all).await.unwrap();
                all
            });
            drain.finish(Duration::from_secs(5)).await;
            let all = reader.await.unwrap();
            let want: Vec<u8> = (0..50u8).flat_map(|n| [n; 40]).collect();
            assert_eq!(all, want);
        });
    }

    #[test]
    fn a_write_is_ready_at_once_even_when_the_reader_is_stuck() {
        run(async {
            let (sink, _far) = tokio::io::duplex(8);
            let (mut writer, _drain) = queued(sink);
            let written =
                tokio::time::timeout(Duration::from_secs(1), writer.write_all(&[7; 1000])).await;
            assert!(matches!(written, Ok(Ok(()))), "{written:?}");
        });
    }

    #[test]
    fn a_client_that_does_not_read_gets_an_error_not_unbounded_memory() {
        run(async {
            let (sink, _far) = tokio::io::duplex(8);
            let (mut writer, _drain) = queued(sink);
            let chunk = vec![1u8; 1024 * 1024];
            let mut failed = false;
            for _ in 0..(MAX_QUEUED_BYTES / chunk.len() + 2) {
                if writer.write_all(&chunk).await.is_err() {
                    failed = true;
                    break;
                }
            }
            assert!(failed, "the queue has no bound");
        });
    }

    #[test]
    fn a_closed_reader_ends_the_writes() {
        run(async {
            let (sink, far) = tokio::io::duplex(8);
            drop(far);
            let (mut writer, drain) = queued(sink);
            let _ = writer.write_all(b"one").await;
            drain.finish(Duration::from_secs(5)).await;
            assert!(writer.write_all(b"two").await.is_err());
        });
    }
}
