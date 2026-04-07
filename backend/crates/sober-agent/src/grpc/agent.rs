//! Core agent RPC handler logic, extracted from [`grpc`].
//!
//! These standalone async functions contain the implementation for the primary
//! agent RPCs: `handle_message`, `execute_task`, and
//! `subscribe_conversation_updates`. The `AgentService` trait impl in `mod.rs`
//! delegates to them, keeping the main trait impl focused and the file
//! manageable.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{Instrument, error};

use super::{AgentGrpcService, proto};
use crate::grpc::content_blocks;
use crate::grpc::tasks;

// ---------------------------------------------------------------------------
// handle_message
// ---------------------------------------------------------------------------

/// Handles a user message: validates IDs, spawns the agent pipeline, and
/// returns a placeholder ack. Events are delivered via the broadcast channel.
pub(crate) async fn handle_message<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::HandleMessageRequest>,
) -> Result<Response<proto::HandleMessageResponse>, Status> {
    // Extract trace context BEFORE creating the span so the OTel layer
    // assigns the correct trace ID (inheriting the caller's trace).
    // The guard must be dropped before any .await (it's !Send).
    let span = {
        let parent_cx = sober_core::extract_trace_context(request.metadata());
        let _guard = parent_cx.attach();
        tracing::info_span!(
            "agent.handle_message",
            otel.kind = "server",
            rpc.service = "AgentService",
            rpc.method = "HandleMessage",
            rpc.system = "grpc",
            user.id = tracing::field::Empty,
            conversation.id = tracing::field::Empty,
            message.length = tracing::field::Empty,
            trigger = "human",
            otel.status_code = tracing::field::Empty,
        )
    };
    let _enter = span.enter();

    let req = request.into_inner();

    let user_id = req
        .user_id
        .parse::<uuid::Uuid>()
        .map(UserId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid user_id"))?;

    let conversation_id = req
        .conversation_id
        .parse::<uuid::Uuid>()
        .map(ConversationId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    span.record("user.id", user_id.to_string().as_str());
    span.record("conversation.id", conversation_id.to_string().as_str());
    span.record("message.length", req.content.len());

    let agent = Arc::clone(service.agent());
    let content_blocks = content_blocks::proto_to_domain(&req.content);
    let source = if req.source == proto::MessageSource::Unspecified as i32 {
        proto::MessageSource::Web as i32
    } else {
        req.source
    };

    match agent
        .handle_message(
            user_id,
            conversation_id,
            &content_blocks,
            sober_core::types::access::TriggerKind::Human,
            source,
        )
        .await
    {
        Ok((user_msg_id, stream)) => {
            span.record("otel.status_code", "OK");
            // The stream must be consumed to drive the spawned task, but
            // we don't need its output — the broadcast channel delivers
            // events. Spawn a drainer task.
            let drainer_span =
                tracing::debug_span!("agent.drain_stream", conversation.id = %conversation_id);
            tokio::spawn(
                async move {
                    use futures::StreamExt;
                    let mut stream = stream;
                    while stream.next().await.is_some() {}
                }
                .instrument(drainer_span),
            );

            Ok(Response::new(proto::HandleMessageResponse {
                message_id: user_msg_id.to_string(),
            }))
        }
        Err(e) => {
            span.record("otel.status_code", "ERROR");
            error!(error.message = %e, "agent handle_message failed");
            Err(Status::internal(e.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// execute_task
// ---------------------------------------------------------------------------

/// Streaming response type for `execute_task`.
pub(crate) type ExecuteTaskStream = ReceiverStream<Result<proto::AgentEvent, Status>>;

/// Handles a scheduled or delegated task. Dispatches to the typed payload
/// pipeline or falls back to the legacy raw-prompt path.
pub(crate) async fn execute_task<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::ExecuteTaskRequest>,
) -> Result<Response<ExecuteTaskStream>, Status> {
    let span = {
        let parent_cx = sober_core::extract_trace_context(request.metadata());
        let _guard = parent_cx.attach();
        tracing::info_span!(
            "agent.execute_task",
            otel.kind = "server",
            rpc.service = "AgentService",
            rpc.method = "ExecuteTask",
            rpc.system = "grpc",
            task.id = tracing::field::Empty,
            task.type = tracing::field::Empty,
            caller = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        )
    };
    let _enter = span.enter();

    let req = request.into_inner();

    span.record("task.id", req.task_id.as_str());
    span.record("task.type", req.task_type.as_str());
    span.record("caller", req.caller_identity.as_str());

    let user_id = req
        .user_id
        .map(|s| {
            s.parse::<uuid::Uuid>()
                .map(UserId::from_uuid)
                .map_err(|_| Status::invalid_argument("invalid user_id"))
        })
        .transpose()?;

    let conversation_id = req
        .conversation_id
        .map(|s| {
            s.parse::<uuid::Uuid>()
                .map(ConversationId::from_uuid)
                .map_err(|_| Status::invalid_argument("invalid conversation_id"))
        })
        .transpose()?;

    let workspace_id = req
        .workspace_id
        .map(|s| {
            s.parse::<uuid::Uuid>()
                .map(WorkspaceId::from_uuid)
                .map_err(|_| Status::invalid_argument("invalid workspace_id"))
        })
        .transpose()?;

    tracing::info!(
        task_id = %req.task_id,
        task_type = %req.task_type,
        caller = %req.caller_identity,
        user_id = ?user_id,
        conversation_id = ?conversation_id,
        workspace_id = ?workspace_id,
        payload_len = req.payload.len(),
        "executing task"
    );

    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let agent = Arc::clone(service.agent());
    let task_id = req.task_id;
    let task_type = req.task_type;
    let payload = req.payload;

    let task_span = tracing::info_span!("agent.execute_task_worker", task.id = %task_id);
    tokio::spawn(
        async move {
            // Try to deserialize as a typed JobPayload; fall back to raw prompt.
            match serde_json::from_slice::<JobPayload>(&payload) {
                Ok(job_payload) => {
                    tasks::execute_typed_payload(
                        &agent,
                        job_payload,
                        user_id,
                        conversation_id,
                        workspace_id,
                        &task_id,
                        &tx,
                    )
                    .await;
                }
                Err(_) => {
                    // Legacy path: treat payload as a UTF-8 prompt string.
                    let prompt = match String::from_utf8(payload) {
                        Ok(s) if !s.is_empty() => s,
                        _ => format!("Execute scheduled task: {task_type} (id: {task_id})"),
                    };

                    tasks::execute_prompt_conversational(
                        &agent,
                        &prompt,
                        user_id,
                        conversation_id,
                        &task_id,
                        &tx,
                    )
                    .await;
                }
            }
        }
        .instrument(task_span),
    );

    Ok(Response::new(ReceiverStream::new(rx)))
}

// ---------------------------------------------------------------------------
// subscribe_conversation_updates
// ---------------------------------------------------------------------------

/// Streaming response type for `subscribe_conversation_updates`.
pub(crate) type SubscribeConversationUpdatesStream =
    ReceiverStream<Result<proto::ConversationUpdate, Status>>;

/// Sets up a subscription that forwards broadcast conversation events to the
/// caller. Runs until the caller disconnects or the broadcast channel closes.
pub(crate) async fn subscribe_conversation_updates<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    _request: Request<proto::SubscribeRequest>,
) -> Result<Response<SubscribeConversationUpdatesStream>, Status> {
    let mut rx = service.broadcast_tx().subscribe();
    let (tx, out_rx) = tokio::sync::mpsc::channel(64);

    let subscription_span = tracing::debug_span!("agent.subscribe_conversation_updates");
    tokio::spawn(
        async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        if tx.send(Ok(update)).await.is_err() {
                            // Client disconnected.
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            skipped = n,
                            "subscription lagged, some events were dropped"
                        );
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
        .instrument(subscription_span),
    );

    Ok(Response::new(ReceiverStream::new(out_rx)))
}
