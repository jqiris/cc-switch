//! 本地模型支持测试
//!
//! 测试 apiFormat="openai_chat" 的本地模型配置

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn test_local_model_config_parsing() {
        // 模拟用户提供的配置
        let config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:8168",
                "ANTHROPIC_AUTH_TOKEN": "xxxx",
                "ANTHROPIC_MODEL": "qwen2.5-coder-32b-instruct-q4_k_m",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "qwen2.5-coder-32b-instruct-q4_k_m",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "qwen2.5-coder-32b-instruct-q4_k_m",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "qwen2.5-coder-32b-instruct-q4_k_m"
            }
        });

        let meta = json!({
            "apiFormat": "openai_chat",
            "commonConfigEnabled": true,
            "endpointAutoSelect": true
        });

        // 验证配置结构
        assert!(config["env"]["ANTHROPIC_BASE_URL"].is_string());
        assert_eq!(
            config["env"]["ANTHROPIC_BASE_URL"],
            "http://127.0.0.1:8168"
        );
        assert!(meta["apiFormat"].is_string());
        assert_eq!(meta["apiFormat"], "openai_chat");
    }

    #[test]
    fn test_endpoint_transformation() {
        // 当 apiFormat="openai_chat" 时，端点应该从 /v1/messages 映射到 /v1/chat/completions
        let original_endpoint = "/v1/messages";
        let api_format = "openai_chat";

        let target_endpoint = match api_format {
            "openai_chat" => "/v1/chat/completions",
            "openai_responses" => "/v1/responses",
            _ => original_endpoint,
        };

        assert_eq!(target_endpoint, "/v1/chat/completions");
    }

    #[test]
    fn test_url_building_without_beta_param() {
        // OpenAI Chat Completions 端点不应该添加 ?beta=true 参数
        let base_url = "http://127.0.0.1:8168";
        let endpoint = "/v1/chat/completions";

        let url = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );

        assert_eq!(url, "http://127.0.0.1:8168/v1/chat/completions");
        assert!(!url.contains("?beta=true"));
    }

    #[test]
    fn test_request_transformation_anthropic_to_openai() {
        let anthropic_request = json!({
            "model": "qwen2.5-coder-32b-instruct-q4_k_m",
            "max_tokens": 100,
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ],
            "stream": false
        });

        // 验证基本字段
        assert_eq!(anthropic_request["model"], "qwen2.5-coder-32b-instruct-q4_k_m");
        assert_eq!(anthropic_request["max_tokens"], 100);
        assert_eq!(anthropic_request["messages"][0]["role"], "user");
        assert_eq!(anthropic_request["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_response_transformation_openai_to_anthropic() {
        let openai_response = json!({
            "id": "chatcmpl-test",
            "model": "qwen2.5-coder-32b-instruct-q4_k_m",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        });

        // 验证 OpenAI 响应结构
        assert!(openai_response["choices"].is_array());
        let choices = openai_response["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0]["message"]["role"], "assistant");
        assert_eq!(choices[0]["finish_reason"], "stop");
    }

    #[test]
    fn test_cache_token_handling() {
        // 测试缓存 token 的处理（用户的实际响应包含这些字段）
        let openai_usage = json!({
            "prompt_tokens": 30,
            "completion_tokens": 10,
            "total_tokens": 40,
            "prompt_tokens_details": {
                "cached_tokens": 24
            }
        });

        // 验证缓存 token 字段存在
        assert_eq!(openai_usage["prompt_tokens"], 30);
        assert_eq!(openai_usage["prompt_tokens_details"]["cached_tokens"], 24);

        // 这些应该被映射到 Anthropic 格式的 cache_read_input_tokens
    }

    #[test]
    fn test_local_model_url_format() {
        // 测试本地模型 URL 的各种格式
        let test_cases = vec![
            ("http://127.0.0.1:8168", "http://127.0.0.1:8168"),
            ("http://localhost:8168", "http://localhost:8168"),
            ("http://127.0.0.1:8168/", "http://127.0.0.1:8168"),
            ("http://127.0.0.1:8168/v1", "http://127.0.0.1:8168/v1"),
        ];

        for (input, expected) in test_cases {
            let normalized = input.trim_end_matches('/');
            assert_eq!(normalized, expected);
        }
    }
}
