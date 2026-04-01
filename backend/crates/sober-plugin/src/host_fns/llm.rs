//! Host function: LLM completion.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, LlmCompleteRequest, LlmCompleteResponse, capability_denied_error,
    not_yet_connected_error, read_input, write_output,
};

/// Default max_tokens when the plugin doesn't specify one.
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// Minimum max_tokens enforced regardless of what the plugin requests.
/// Thinking models need headroom for reasoning before producing content.
const MIN_MAX_TOKENS: u32 = 4096;

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
    let system_prompt = ctx.system_prompt.clone();

    let mut messages = Vec::new();
    if !req.raw
        && let Some(sp) = &system_prompt
    {
        messages.push(sober_llm::Message::system(sp));
    }
    messages.push(sober_llm::Message::user(req.prompt));

    let completion_req = sober_llm::CompletionRequest {
        model,
        messages,
        tools: vec![],
        max_tokens: Some(
            req.max_tokens
                .unwrap_or(DEFAULT_MAX_TOKENS)
                .max(MIN_MAX_TOKENS),
        ),
        temperature: None,
        stop: vec![],
        stream: false,
    };

    tracing::info!(
        max_tokens = ?completion_req.max_tokens,
        model = %completion_req.model,
        message_count = completion_req.messages.len(),
        "plugin LLM request"
    );

    // Drop the lock before blocking (see lock discipline note in host_fns/mod.rs).
    drop(ctx);

    let response = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?
        .block_on_async(engine.complete(completion_req))?
        .map_err(|e| extism::Error::msg(format!("llm complete failed: {e}")))?;

    tracing::info!(
        choices_count = response.choices.len(),
        first_content = ?response.choices.first().and_then(|c| c.message.text_content()),
        first_reasoning = ?response.choices.first().and_then(|c| c.message.reasoning_content.as_deref()),
        first_role = ?response.choices.first().map(|c| &c.message.role),
        finish_reason = ?response.choices.first().map(|c| &c.finish_reason),
        "plugin LLM response debug"
    );

    let choice = response.choices.into_iter().next();
    let text = choice
        .as_ref()
        .and_then(|c| c.message.text_content())
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_owned();

    if text.is_empty() {
        let reason = choice
            .as_ref()
            .and_then(|c| c.finish_reason.as_deref())
            .unwrap_or("unknown");
        tracing::warn!(
            finish_reason = reason,
            "LLM returned empty content for plugin call"
        );
    }

    write_output(plugin, outputs, &LlmCompleteResponse { text })
}
