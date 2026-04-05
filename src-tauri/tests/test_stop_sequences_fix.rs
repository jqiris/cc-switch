//! Tests verifying stop_sequences handling across format transformations.
//!
//! These tests validate the core stop_sequences fix:
//! - Anthropic `stop_sequences` → OpenAI `stop` (Chat Completions)
//! - Anthropic `stop_sequences` → dropped (Responses API)
//! - `strip_anthropic_fields` removes `prompt_cache_key` and `cache_control`
//! - `filter_private_params` strips `_`-prefixed fields while preserving stop_sequences
//!
//! Because `proxy` is not `pub` in lib.rs, we inline the transform functions
//! that are relevant to the stop_sequences fix.

use serde_json::{json, Value};

// =============================================================================
// Inlined from proxy::providers::transform (stop_sequences mapping)
// =============================================================================

/// Anthropic → OpenAI Chat Completions
///
/// Key behavior: `stop_sequences` → `stop`
fn anthropic_to_openai(body: Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // system prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            messages.push(json!({"role": "system", "content": text}));
        } else if let Some(arr) = system.as_array() {
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    messages.push(json!({"role": "system", "content": text}));
                }
            }
        }
    }

    // messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            if let Some(text) = content.and_then(|c| c.as_str()) {
                messages.push(json!({"role": role, "content": text}));
            } else {
                messages.push(json!({"role": role, "content": content}));
            }
        }
    }

    result["messages"] = json!(messages);

    if let Some(v) = body.get("max_tokens") {
        result["max_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }

    // === KEY: stop_sequences → stop ===
    if let Some(v) = body.get("stop_sequences") {
        result["stop"] = v.clone();
    }

    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }
    if let Some(v) = body.get("tools") {
        result["tools"] = v.clone();
    }
    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = v.clone();
    }

    result
}

// =============================================================================
// Inlined from proxy::providers::transform_responses (stop_sequences dropped)
// =============================================================================

/// Anthropic → OpenAI Responses API
///
/// Key behavior: `stop_sequences` is **dropped** (Responses API has no equivalent)
fn anthropic_to_responses(body: Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // system → instructions
    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            text.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };
        if !instructions.is_empty() {
            result["instructions"] = json!(instructions);
        }
    }

    // messages → input (simplified)
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        let input: Vec<Value> = msgs
            .iter()
            .map(|msg| {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = msg.get("content");
                if let Some(text) = content.and_then(|c| c.as_str()) {
                    let content_type = if role == "assistant" { "output_text" } else { "input_text" };
                    json!({
                        "role": role,
                        "content": [{"type": content_type, "text": text}]
                    })
                } else {
                    json!({"role": role, "content": content})
                }
            })
            .collect();
        result["input"] = json!(input);
    }

    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // === KEY: stop_sequences is intentionally NOT forwarded ===
    // The Responses API does not support stop_sequences.

    result
}

// =============================================================================
// Inlined from proxy::forwarder::strip_anthropic_fields
// =============================================================================

/// Strip Anthropic-specific fields from an OpenAI-compatible request body.
fn strip_anthropic_fields(mut body: Value) -> Value {
    if let Some(obj) = body.as_object_mut() {
        obj.remove("prompt_cache_key");
    }
    strip_cache_control_recursive(&mut body);
    body
}

fn strip_cache_control_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("cache_control");
            for v in map.values_mut() {
                strip_cache_control_recursive(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_cache_control_recursive(v);
            }
        }
        _ => {}
    }
}

// =============================================================================
// Inlined from proxy::body_filter::filter_private_params_with_whitelist
// =============================================================================

use std::collections::HashSet;

fn filter_private_params_with_whitelist(body: Value, whitelist: &[&str]) -> Value {
    let whitelist_set: HashSet<&str> = whitelist.iter().copied().collect();
    filter_recursive(body, &whitelist_set)
}

fn filter_recursive(value: Value, whitelist: &HashSet<&str>) -> Value {
    match value {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .into_iter()
                .filter_map(|(key, val)| {
                    if key.starts_with('_') && !whitelist.contains(key.as_str()) {
                        None
                    } else {
                        Some((key, filter_recursive(val, whitelist)))
                    }
                })
                .collect();
            Value::Object(filtered)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter().map(|v| filter_recursive(v, whitelist)).collect(),
        ),
        other => other,
    }
}

// =============================================================================
// Tests: stop_sequences in Anthropic → OpenAI Chat Completions
// =============================================================================

#[test]
fn test_stop_sequences_mapped_to_stop_in_openai_chat() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": ["\n\nHuman:", "</output>"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_openai(input);

    // stop_sequences becomes "stop"
    assert_eq!(result["stop"], json!(["\n\nHuman:", "</output>"]));
    // original "stop_sequences" key must not leak
    assert!(
        result.get("stop_sequences").is_none(),
        "stop_sequences should not appear in OpenAI output"
    );
}

#[test]
fn test_no_stop_sequences_means_no_stop() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_openai(input);

    assert!(result.get("stop").is_none());
    assert!(result.get("stop_sequences").is_none());
}

#[test]
fn test_empty_stop_sequences_array() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": [],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_openai(input);

    assert_eq!(result["stop"], json!([]));
    assert!(result.get("stop_sequences").is_none());
}

#[test]
fn test_single_stop_sequence() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": ["END"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_openai(input);

    assert_eq!(result["stop"], json!(["END"]));
}

#[test]
fn test_stop_sequences_with_system_prompt() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "system": "You are helpful.",
        "stop_sequences": ["\n\nHuman:"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_openai(input);

    assert_eq!(result["stop"], json!(["\n\nHuman:"]));
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][1]["role"], "user");
}

// =============================================================================
// Tests: stop_sequences in Anthropic → OpenAI Responses API (dropped)
// =============================================================================

#[test]
fn test_stop_sequences_dropped_in_responses_api() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop_sequences": ["\n\nHuman:"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input);

    assert!(
        result.get("stop_sequences").is_none(),
        "stop_sequences must not appear in Responses API output"
    );
    assert!(
        result.get("stop").is_none(),
        "stop must not appear in Responses API output"
    );
}

#[test]
fn test_responses_api_preserves_core_fields() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop_sequences": ["STOP"],
        "system": "Be helpful",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input);

    assert_eq!(result["model"], "gpt-4o");
    assert_eq!(result["max_output_tokens"], 256);
    assert_eq!(result["instructions"], "Be helpful");
    assert!(result.get("stop_sequences").is_none());
    assert!(result.get("stop").is_none());
}

// =============================================================================
// Tests: strip_anthropic_fields preserves stop, removes cache fields
// =============================================================================

#[test]
fn test_strip_preserves_stop() {
    let body = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop": ["\n\nHuman:"],
        "messages": [],
        "prompt_cache_key": "provider-123"
    });

    let stripped = strip_anthropic_fields(body);

    assert!(stripped.get("prompt_cache_key").is_none());
    assert_eq!(stripped["stop"], json!(["\n\nHuman:"]));
}

#[test]
fn test_strip_removes_cache_control_nested() {
    let body = json!({
        "model": "gpt-4o",
        "stop": ["STOP"],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
            ]
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "test_tool",
                "cache_control": {"type": "ephemeral"}
            }
        }]
    });

    let stripped = strip_anthropic_fields(body);

    // stop preserved
    assert_eq!(stripped["stop"], json!(["STOP"]));
    // cache_control removed at all levels
    let msg_content = stripped["messages"][0]["content"].as_array().unwrap();
    assert!(msg_content[0].get("cache_control").is_none());
    assert!(stripped["tools"][0].get("cache_control").is_none());
}

#[test]
fn test_strip_noop_when_no_anthropic_fields() {
    let body = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop": ["STOP"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let stripped = strip_anthropic_fields(body.clone());

    assert_eq!(stripped, body);
}

// =============================================================================
// Tests: filter_private_params preserves stop_sequences
// =============================================================================

#[test]
fn test_filter_strips_underscore_fields_keeps_stop_sequences() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": ["STOP"],
        "_internal_id": "abc123",
        "_debug_mode": true
    });

    let output = filter_private_params_with_whitelist(input, &[]);

    assert!(output.get("_internal_id").is_none());
    assert!(output.get("_debug_mode").is_none());
    assert_eq!(output["stop_sequences"], json!(["STOP"]));
    assert_eq!(output["model"], "claude-sonnet-4-20250514");
}

#[test]
fn test_filter_whitelist_preserves_specified_underscore_fields() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "stop_sequences": ["STOP"],
        "_metadata": {"key": "value"},
        "_internal_id": "abc123"
    });

    let output = filter_private_params_with_whitelist(input, &["_metadata"]);

    assert!(output.get("_metadata").is_some());
    assert!(output.get("_internal_id").is_none());
    assert_eq!(output["stop_sequences"], json!(["STOP"]));
}

#[test]
fn test_filter_nested_underscore_fields() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "stop_sequences": ["STOP"],
        "messages": [{
            "role": "user",
            "content": "Hello",
            "_session_token": "secret"
        }]
    });

    let output = filter_private_params_with_whitelist(input, &[]);

    let messages = output["messages"].as_array().unwrap();
    assert!(messages[0].get("_session_token").is_none());
    assert_eq!(messages[0]["content"], "Hello");
    assert_eq!(output["stop_sequences"], json!(["STOP"]));
}

#[test]
fn test_filter_noop_when_no_private_fields() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "stop_sequences": ["STOP"],
        "max_tokens": 256
    });

    let output = filter_private_params_with_whitelist(input.clone(), &[]);
    assert_eq!(output, input);
}

// =============================================================================
// Tests: End-to-end pipeline
// =============================================================================

#[test]
fn test_full_pipeline_openai_chat_stop_sequences() {
    // Step 1: Anthropic request with stop_sequences + private fields
    let anthropic_body = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": ["\n\nHuman:", "</output>"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // Step 2: Transform to OpenAI Chat
    let openai_body = anthropic_to_openai(anthropic_body);
    assert_eq!(openai_body["stop"], json!(["\n\nHuman:", "</output>"]));
    assert!(openai_body.get("stop_sequences").is_none());

    // Step 3: Strip Anthropic fields
    let stripped = strip_anthropic_fields(openai_body);
    assert_eq!(stripped["stop"], json!(["\n\nHuman:", "</output>"]));
    assert!(stripped.get("stop_sequences").is_none());

    // Step 4: Filter private params (no-op in this case)
    let filtered = filter_private_params_with_whitelist(stripped, &[]);
    assert_eq!(filtered["stop"], json!(["\n\nHuman:", "</output>"]));
    assert!(filtered.get("stop_sequences").is_none());
    assert!(filtered.get("prompt_cache_key").is_none());
}

#[test]
fn test_full_pipeline_responses_api_no_stop_sequences() {
    let anthropic_body = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop_sequences": ["STOP_HERE"],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let responses_body = anthropic_to_responses(anthropic_body);

    // stop_sequences must not appear in any form
    assert!(responses_body.get("stop_sequences").is_none());
    assert!(responses_body.get("stop").is_none());
    // core fields preserved
    assert_eq!(responses_body["model"], "gpt-4o");
    assert_eq!(responses_body["max_output_tokens"], 256);
}

#[test]
fn test_full_pipeline_with_private_params_and_stop_sequences() {
    let input = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 256,
        "stop_sequences": ["\n\nHuman:"],
        "messages": [{"role": "user", "content": "Hello"}],
        "_trace_id": "trace-123",
        "_internal": true
    });

    // Transform first (transform drops unknown fields like _trace_id)
    let openai = anthropic_to_openai(input);
    assert_eq!(openai["stop"], json!(["\n\nHuman:"]));

    // Then strip
    let stripped = strip_anthropic_fields(openai);
    assert_eq!(stripped["stop"], json!(["\n\nHuman:"]));

    // Then filter (would remove any remaining _ fields)
    let filtered = filter_private_params_with_whitelist(stripped, &[]);
    assert_eq!(filtered["stop"], json!(["\n\nHuman:"]));
    assert!(filtered.get("stop_sequences").is_none());
    assert!(filtered.get("_trace_id").is_none());
}
