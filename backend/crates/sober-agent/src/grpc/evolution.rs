//! gRPC handler functions for evolution execute/revert RPCs.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::ids::EvolutionEventId;
use sober_core::types::repo::EvolutionRepo;
use tonic::{Request, Response, Status};

use super::AgentGrpcService;
use super::proto;

/// Builds an [`EvolutionContext`] from the gRPC service's dependencies.
fn build_context<R: AgentRepos>(
    service: &AgentGrpcService<R>,
) -> Result<crate::evolution::EvolutionContext<R>, Status> {
    let plugin_generator = service
        .agent()
        .tool_bootstrap()
        .plugin_generator
        .clone()
        .ok_or_else(|| Status::unavailable("plugin generator not configured"))?;

    Ok(crate::evolution::EvolutionContext {
        scheduler_client: Arc::clone(&service.agent().tool_bootstrap().scheduler_client),
        plugin_manager: Arc::clone(&service.plugin_manager),
        plugin_generator,
        evolution_config: service.agent().tool_bootstrap().evolution_config.clone(),
    })
}

/// Parses an evolution event ID from the request string.
fn parse_event_id(raw: &str) -> Result<EvolutionEventId, Status> {
    raw.parse::<uuid::Uuid>()
        .map(EvolutionEventId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid evolution_event_id"))
}

/// Handles the `ExecuteEvolution` RPC.
pub(crate) async fn handle_execute<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::ExecuteEvolutionRequest>,
) -> Result<Response<proto::ExecuteEvolutionResponse>, Status> {
    let event_id = parse_event_id(&request.into_inner().evolution_event_id)?;

    let event = service
        .agent()
        .repos()
        .evolution()
        .get_by_id(event_id)
        .await
        .map_err(|e| Status::not_found(format!("evolution event not found: {e}")))?;

    let ctx = build_context(service)?;

    match crate::evolution::execute_evolution(
        &event,
        service.agent().repos(),
        service.agent().mind(),
        &ctx,
    )
    .await
    {
        Ok(()) => Ok(Response::new(proto::ExecuteEvolutionResponse {
            success: true,
            error: String::new(),
        })),
        Err(e) => Ok(Response::new(proto::ExecuteEvolutionResponse {
            success: false,
            error: e.to_string(),
        })),
    }
}

/// Handles the `RevertEvolution` RPC.
pub(crate) async fn handle_revert<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::RevertEvolutionRequest>,
) -> Result<Response<proto::RevertEvolutionResponse>, Status> {
    let event_id = parse_event_id(&request.into_inner().evolution_event_id)?;

    let event = service
        .agent()
        .repos()
        .evolution()
        .get_by_id(event_id)
        .await
        .map_err(|e| Status::not_found(format!("evolution event not found: {e}")))?;

    let ctx = build_context(service)?;

    match crate::evolution::revert_evolution(
        &event,
        service.agent().repos(),
        service.agent().mind(),
        &ctx,
    )
    .await
    {
        Ok(()) => Ok(Response::new(proto::RevertEvolutionResponse {
            success: true,
            error: String::new(),
        })),
        Err(e) => Ok(Response::new(proto::RevertEvolutionResponse {
            success: false,
            error: e.to_string(),
        })),
    }
}
