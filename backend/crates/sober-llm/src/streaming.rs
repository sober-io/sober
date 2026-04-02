//! SSE parser for OpenAI-compatible streaming responses.
//!
//! The OpenAI streaming format sends Server-Sent Events where each event is:
//! ```text
//! data: {"id":"...","choices":[...]}\n\n
//! ```
//! The stream ends with `data: [DONE]\n\n`.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use futures::stream::{self, Stream};
use metrics::{counter, histogram};
use reqwest::Response;
use tracing::{instrument, warn};

use crate::error::LlmError;
use crate::types::StreamChunk;

/// Parse an SSE response from an OpenAI-compatible provider into a stream of
/// [`StreamChunk`]s.
#[instrument(level = "debug", skip(response))]
pub fn parse_sse_stream(
    response: Response,
) -> impl Stream<Item = Result<StreamChunk, LlmError>> + Send {
    let byte_stream = response.bytes_stream();

    stream::unfold(
        (byte_stream, String::new()),
        |(mut byte_stream, mut buffer)| async move {
            use futures::StreamExt;

            loop {
                // Try to extract a complete SSE event from the buffer.
                if let Some(event) = extract_sse_event(&mut buffer) {
                    if event == "[DONE]" {
                        return None;
                    }

                    match serde_json::from_str::<StreamChunk>(&event) {
                        Ok(chunk) => return Some((Ok(chunk), (byte_stream, buffer))),
                        Err(e) => {
                            warn!(event = %event, error = %e, "skipping malformed SSE chunk");
                            continue;
                        }
                    }
                }

                // Need more data from the network.
                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(e)) => {
                        return Some((Err(LlmError::NetworkError(e)), (byte_stream, buffer)));
                    }
                    None => {
                        // Stream ended without [DONE] — some providers do this.
                        return None;
                    }
                }
            }
        },
    )
}

/// A stream wrapper that records LLM metrics when the inner SSE stream ends.
///
/// Tracks token usage from the final chunk (which carries `usage` stats) and
/// records request duration, token counters, and request total on completion.
pub struct MeteredSseStream<S> {
    inner: Pin<Box<S>>,
    provider: String,
    model: String,
    start: Instant,
    last_usage: Option<crate::types::Usage>,
    done: bool,
}

impl<S> MeteredSseStream<S>
where
    S: Stream<Item = Result<StreamChunk, LlmError>> + Send,
{
    /// Wrap an inner SSE stream with metric recording.
    pub fn new(inner: S, provider: String, model: String, start: Instant) -> Self {
        Self {
            inner: Box::pin(inner),
            provider,
            model,
            start,
            last_usage: None,
            done: false,
        }
    }

    /// Record accumulated metrics when the stream terminates.
    fn record_metrics(&self, status: &str) {
        let elapsed = self.start.elapsed().as_secs_f64();
        counter!("sober_llm_request_total", "provider" => self.provider.clone(), "model" => self.model.clone(), "status" => status.to_owned()).increment(1);
        histogram!("sober_llm_request_duration_seconds", "provider" => self.provider.clone(), "model" => self.model.clone()).record(elapsed);

        if let Some(ref usage) = self.last_usage {
            counter!("sober_llm_tokens_input_total", "provider" => self.provider.clone(), "model" => self.model.clone()).increment(u64::from(usage.prompt_tokens));
            counter!("sober_llm_tokens_output_total", "provider" => self.provider.clone(), "model" => self.model.clone()).increment(u64::from(usage.completion_tokens));
        }
    }
}

impl<S> Stream for MeteredSseStream<S>
where
    S: Stream<Item = Result<StreamChunk, LlmError>> + Send,
{
    type Item = Result<StreamChunk, LlmError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }

        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                // Capture usage from the final chunk (the last one with usage stats).
                if chunk.usage.is_some() {
                    this.last_usage = chunk.usage;
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                this.done = true;
                this.record_metrics("error");
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                this.done = true;
                this.record_metrics("success");
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Extract the next complete SSE `data:` payload from the buffer.
///
/// Returns `Some(payload)` if a complete event is found, removing it from the
/// buffer. Returns `None` if more data is needed.
fn extract_sse_event(buffer: &mut String) -> Option<String> {
    // SSE events are separated by double newlines.
    while let Some(pos) = buffer.find("\n\n") {
        let event_block = buffer[..pos].to_owned();
        // Remove the event + the double newline separator.
        buffer.drain(..pos + 2);

        // Process lines in the event block, looking for `data:` lines.
        for line in event_block.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with(':') {
                // Empty lines and comments — skip.
                continue;
            }

            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() {
                    return Some(data.to_owned());
                }
            }

            // Ignore `event:`, `id:`, `retry:` lines — some providers send these.
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse raw SSE text into stream chunks using `extract_sse_event`.
    fn parse_sse_events(raw: &str) -> Vec<StreamChunk> {
        let mut buffer = raw.to_owned();
        let mut chunks = Vec::new();

        while let Some(event) = extract_sse_event(&mut buffer) {
            if event == "[DONE]" {
                break;
            }
            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(&event) {
                chunks.push(chunk);
            }
        }

        chunks
    }

    #[test]
    fn parse_text_only_stream() {
        let raw = concat!(
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
            "data: [DONE]\n\n",
        );

        let chunks = parse_sse_events(raw);

        assert_eq!(chunks.len(), 4);
        assert_eq!(
            chunks[0].choices[0].delta.role.as_deref(),
            Some("assistant")
        );
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("Hello"));
        assert_eq!(
            chunks[2].choices[0].delta.content.as_deref(),
            Some(" world")
        );
        assert_eq!(chunks[3].choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(chunks[3].usage.is_some());
    }

    #[test]
    fn parse_tool_call_stream() {
        let raw = concat!(
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"London\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );

        let chunks = parse_sse_events(raw);

        assert_eq!(chunks.len(), 4);
        let first_tc = &chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(first_tc.id.as_deref(), Some("call_1"));
        assert_eq!(
            first_tc.function.as_ref().unwrap().name.as_deref(),
            Some("get_weather")
        );
    }

    #[test]
    fn handles_done_terminates_parsing() {
        let raw = concat!(
            "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        );

        let chunks = parse_sse_events(raw);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn extract_sse_event_basic() {
        let mut buffer = "data: {\"test\": true}\n\n".to_owned();
        let event = extract_sse_event(&mut buffer);
        assert_eq!(event.as_deref(), Some("{\"test\": true}"));
        assert!(buffer.is_empty());
    }

    #[test]
    fn extract_sse_event_skips_comments_and_empty_lines() {
        let mut buffer = ": comment\n\ndata: hello\n\n".to_owned();
        let event = extract_sse_event(&mut buffer);
        assert_eq!(event.as_deref(), Some("hello"));
    }

    #[test]
    fn extract_sse_event_incomplete_returns_none() {
        let mut buffer = "data: partial".to_owned();
        assert!(extract_sse_event(&mut buffer).is_none());
    }
}
