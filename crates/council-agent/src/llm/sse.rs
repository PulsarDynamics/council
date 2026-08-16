//! Minimal Server-Sent Events (SSE) parser built on top of a `reqwest`
//! response byte stream.
//!
//! SSE wire format (RFC: <https://www.w3.org/TR/eventsource/>):
//!
//! - The response is `text/event-stream`, kept open by the server.
//! - Events are separated by a blank line (`\n\n` or `\r\n\r\n`).
//! - Inside one event, lines look like `field: value`; we only care about
//!   `data:` lines. A single event can have multiple `data:` lines; per
//!   the spec they get joined with newlines, but in practice every
//!   provider we talk to (OpenAI Chat, OpenAI Responses, Anthropic)
//!   emits exactly one `data:` line per event, and that line carries
//!   a single JSON object — or, for the OpenAI Chat terminator, the
//!   literal `[DONE]`.
//! - The stream ends when the provider closes the connection (or
//!   immediately after the explicit `[DONE]` payload on OpenAI Chat).
//!
//! Our `SseStream` wraps a `Stream<Item = Result<Bytes, reqwest::Error>>`
//! and yields one `SseEvent { data: String }` per parsed event. Empty
//! events (heartbeats, comments) are skipped. Bytes that don't yet form
//! a complete event are buffered until the next chunk arrives.

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// One decoded SSE event: the concatenated `data:` payload as a string.
/// Callers parse it as JSON (or check for `[DONE]` for the OpenAI Chat
/// terminator).
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub data: String,
}

pub struct SseStream<S> {
    inner: S,
    /// Bytes we've read from `inner` but haven't yet parsed into a
    /// complete event. We scan this for the ASCII byte sequences that
    /// delimit events, so partial multi-byte UTF-8 characters across
    /// chunks are safe.
    buffer: Vec<u8>,
    /// Set once `inner` returns `Ready(None)`; we still drain any
    /// remaining buffered event(s) before reporting end-of-stream.
    inner_done: bool,
}

impl<S> SseStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            inner_done: false,
        }
    }
}

impl<S> Stream for SseStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<SseEvent, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // 1. Look for a complete event in the buffer.
            if let Some(end) = find_event_boundary(&self.buffer) {
                let raw: Vec<u8> = self.buffer.drain(..end).collect();
                let event_str = match std::str::from_utf8(&raw) {
                    Ok(s) => s,
                    Err(_e) => {
                        // Bad UTF-8 in one event: skip it. The byte
                        // sequence is otherwise well-formed (we found
                        // the event separator), so it's most likely a
                        // truncated multi-byte char at the boundary —
                        // not worth killing the whole stream over.
                        continue;
                    }
                };
                let data = collect_data_lines(event_str);
                if data.is_empty() {
                    // Comment-only or empty event — keep looking.
                    continue;
                }
                return Poll::Ready(Some(Ok(SseEvent { data })));
            }

            // 2. If the inner stream is done and there's nothing left,
            //    we're finished.
            if self.inner_done {
                if self.buffer.is_empty() {
                    return Poll::Ready(None);
                }
                // Drain whatever is left as a final (possibly partial) event.
                let raw: Vec<u8> = self.buffer.drain(..).collect();
                let event_str = String::from_utf8_lossy(&raw).to_string();
                let data = collect_data_lines(&event_str);
                if data.is_empty() {
                    return Poll::Ready(None);
                }
                return Poll::Ready(Some(Ok(SseEvent { data })));
            }

            // 3. Otherwise, pull more bytes from the inner stream.
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.buffer.extend_from_slice(&chunk);
                    // Loop and re-check for a complete event.
                }
                Poll::Ready(Some(Err(e))) => {
                    self.inner_done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    self.inner_done = true;
                    // Loop; we'll either yield the tail buffer or finish.
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// If `buf` contains a complete event separator, return the index *just
/// past* the separator (so `buf[..idx]` is the event payload, including
/// the separator). Returns `None` if the separator isn't fully present
/// yet. Accepts both `\n\n` and `\r\n\r\n`; in practice every provider
/// we talk to uses `\n\n`.
fn find_event_boundary(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..buf.len() - 1 {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i + 2);
        }
    }
    if buf.len() >= 4 {
        let mut i = 0;
        while i + 4 <= buf.len() {
            if &buf[i..i + 4] == b"\r\n\r\n" {
                return Some(i + 4);
            }
            i += 1;
        }
    }
    None
}

/// Concatenate the `data:` lines of one event into a single string.
/// We strip the `data:` prefix and the optional leading space; we do
/// not insert a `\n` between lines because every provider we handle
/// uses exactly one `data:` line per event.
fn collect_data_lines(event: &str) -> String {
    let mut out = String::new();
    for line in event.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            let piece = rest.strip_prefix(' ').unwrap_or(rest);
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(piece);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn chunk(s: &str) -> Result<Bytes, reqwest::Error> {
        Ok(Bytes::from(s.to_string()))
    }

    fn collect<S>(s: S) -> Vec<SseEvent>
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
    {
        let sse = SseStream::new(s);
        futures::executor::block_on(async {
            futures::StreamExt::collect::<Vec<_>>(sse)
                .await
                .into_iter()
                .map(|r| r.unwrap())
                .collect()
        })
    }

    #[test]
    fn parses_single_event() {
        let ev = collect(stream::iter(vec![chunk("data: {\"a\":1}\n\n")]));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].data, "{\"a\":1}");
    }

    #[test]
    fn parses_split_chunks() {
        let ev = collect(stream::iter(vec![
            chunk("data: {\"a\""),
            chunk(":1}\n\ndata: [DONE]\n\n"),
        ]));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].data, "{\"a\":1}");
        assert_eq!(ev[1].data, "[DONE]");
    }

    #[test]
    fn handles_two_events_in_one_chunk() {
        let ev = collect(stream::iter(vec![chunk("data: a\n\ndata: b\n\n")]));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].data, "a");
        assert_eq!(ev[1].data, "b");
    }

    #[test]
    fn skips_heartbeat_lines() {
        // `: keepalive` is a comment per the SSE spec — our parser should
        // skip the event entirely because it has no `data:` line.
        let ev = collect(stream::iter(vec![chunk(
            ": keepalive\n\ndata: {\"x\":1}\n\n",
        )]));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].data, "{\"x\":1}");
    }

    #[test]
    fn handles_crlf_separator() {
        let ev = collect(stream::iter(vec![chunk("data: a\r\n\r\ndata: b\r\n\r\n")]));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].data, "a");
        assert_eq!(ev[1].data, "b");
    }

    #[test]
    fn handles_tail_without_trailing_newline() {
        // The connection closes without a final blank line — we should
        // still yield the last event.
        let ev = collect(stream::iter(vec![chunk("data: only\n\ndata: last")]));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].data, "only");
        assert_eq!(ev[1].data, "last");
    }
}
