//! Host function: LLM completion.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, LlmCompleteRequest, LlmCompleteResponse, capability_denied_error,
    not_yet_connected_error, read_input, write_output,
};

/// Sends a prompt to an LLM provider.
///
/// Requires the `LlmCall` capability.  Returns an error if no LLM engine is
/// configured.
pub(crate) fn host_llm_complete_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: LlmCompleteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::LlmCall) {
        return capability_denied_error(plugin, outputs, "llm_call");
    }

    let engine = match &ctx.llm_engine {
        Some(e) => Arc::clone(e),
        None => return not_yet_connected_error(plugin, outputs, "host_llm_complete"),
    };

    let model = req.model.unwrap_or_else(|| engine.model_id().to_string());

    let completion_req = sober_llm::CompletionRequest {
        model,
        messages: vec![sober_llm::Message::user(req.prompt)],
        tools: vec![],
        max_tokens: req.max_tokens,
        temperature: None,
        stop: vec![],
        stream: false,
    };

    // Drop the lock before blocking (see lock discipline note in host_fns/mod.rs).
    drop(ctx);

    let response = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?
        .block_on_async(engine.complete(completion_req))?
        .map_err(|e| extism::Error::msg(format!("llm complete failed: {e}")))?;

    let text = response
        .choices
        .into_iter()
        .next()
        .map(|c| {
            c.message
                .content
                .or(c.message.reasoning_content)
                .unwrap_or_default()
        })
        .unwrap_or_default();

    if text.is_empty() {
        tracing::warn!("LLM returned empty content for plugin call");
    }

    write_output(plugin, outputs, &LlmCompleteResponse { text })
}
