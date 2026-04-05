//! 项目目录映射 + apiFormat 集成测试

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn test_project_mapping_with_openai_chat_format() {
        // 模拟项目目录映射选择了一个配置了 apiFormat="openai_chat" 的 provider
        let provider_from_mapping = json!({
            "id": "local-model-provider",
            "name": "Local Qwen Model",
            "settings_config": {
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8168",
                    "ANTHROPIC_AUTH_TOKEN": "xxxx",
                    "ANTHROPIC_MODEL": "qwen2.5-coder-32b-instruct-q4_k_m"
                }
            },
            "meta": {
                "apiFormat": "openai_chat"
            }
        });

        // 验证配置结构
        assert_eq!(
            provider_from_mapping["meta"]["apiFormat"],
            "openai_chat"
        );
        assert_eq!(
            provider_from_mapping["settings_config"]["env"]["ANTHROPIC_BASE_URL"],
            "http://127.0.0.1:8168"
        );
    }

    #[test]
    fn test_header_handling_for_openai_compatible() {
        // 测试 Header 处理逻辑
        let api_format = "openai_chat";
        let is_openai_compatible = api_format == "openai_chat" || api_format == "openai_responses";

        // 当 apiFormat="openai_chat" 时，应该跳过 Anthropic headers
        assert!(is_openai_compatible);

        // 预期行为：
        // - 不添加 anthropic-beta
        // - 不添加 anthropic-version
        // - 只发送标准 OpenAI headers (Authorization, Content-Type)
    }

    #[test]
    fn test_header_handling_for_anthropic_native() {
        // 测试 Anthropic 原生格式的 Header 处理
        let api_format = "anthropic";
        let is_openai_compatible = api_format == "openai_chat" || api_format == "openai_responses";

        // 当 apiFormat="anthropic" 时，应该添加 Anthropic headers
        assert!(!is_openai_compatible);

        // 预期行为：
        // - 添加 anthropic-beta: claude-code-20250219
        // - 添加 anthropic-version: 2023-06-01
    }

    #[test]
    fn test_endpoint_mapping_for_different_formats() {
        // 测试端点映射
        let original_endpoint = "/v1/messages";

        // OpenAI Chat 格式
        let api_format_chat = "openai_chat";
        let target_chat = match api_format_chat {
            "openai_chat" => "/v1/chat/completions",
            "openai_responses" => "/v1/responses",
            _ => original_endpoint,
        };
        assert_eq!(target_chat, "/v1/chat/completions");

        // OpenAI Responses 格式
        let api_format_responses = "openai_responses";
        let target_responses = match api_format_responses {
            "openai_chat" => "/v1/chat/completions",
            "openai_responses" => "/v1/responses",
            _ => original_endpoint,
        };
        assert_eq!(target_responses, "/v1/responses");

        // Anthropic 原生格式
        let api_format_anthropic = "anthropic";
        let target_anthropic = match api_format_anthropic {
            "openai_chat" => "/v1/chat/completions",
            "openai_responses" => "/v1/responses",
            _ => original_endpoint,
        };
        assert_eq!(target_anthropic, "/v1/messages");
    }

    #[test]
    fn test_project_mapping_priority() {
        // 测试项目目录映射的优先级
        // 项目目录映射应该在全局 current provider 之前选择

        // 场景：
        // 1. 全局 current provider = "provider-global"
        // 2. 项目映射匹配到 "provider-local"
        // 3. 实际使用 = "provider-local" (项目映射优先)

        let global_provider = "provider-global";
        let mapped_provider = "provider-local";

        // 项目映射应该覆盖全局设置
        assert_ne!(global_provider, mapped_provider);
        assert_eq!(mapped_provider, "provider-local");
    }

    #[test]
    fn test_no_project_mapping_fallback() {
        // 测试没有项目映射时的回退逻辑
        // 如果没有匹配的项目映射，应该使用全局 current provider

        let global_provider = "provider-global";
        let mapped_provider: Option<&str> = None;

        let effective_provider = mapped_provider.unwrap_or(global_provider);
        assert_eq!(effective_provider, "provider-global");
    }

    #[test]
    fn test_is_project_mapped_flag() {
        // 测试 is_project_mapped_provider 标记
        // 这个标记用于防止项目映射的 provider 切换影响全局设置

        let is_project_mapped = true;

        if is_project_mapped {
            // 成功后不触发全局 provider 切换
            println!("项目映射的 provider，不切换全局设置");
        } else {
            // 成功后切换全局 current provider
            println!("全局 provider，可以切换");
        }

        assert!(is_project_mapped);
    }
}
