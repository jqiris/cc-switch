//! 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换请求中的模型名称
//! 支持智能路由：根据请求复杂度自动选择合适的模型层级

use crate::provider::Provider;
use serde_json::Value;

/// 模型映射配置
pub struct ModelMapping {
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub default_model: Option<String>,
    pub reasoning_model: Option<String>,
}

impl ModelMapping {
    /// 从 Provider 配置中提取模型映射
    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");

        Self {
            haiku_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            reasoning_model: env
                .and_then(|e| e.get("ANTHROPIC_REASONING_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了任何模型映射
    pub fn has_mapping(&self) -> bool {
        self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.default_model.is_some()
            || self.reasoning_model.is_some()
    }

    /// 检查是否配置了智能路由所需的层级模型
    pub fn has_tier_models(&self) -> bool {
        self.haiku_model.is_some() || self.sonnet_model.is_some() || self.opus_model.is_some()
    }

    /// 根据层级获取模型
    pub fn get_model_for_tier(
        &self,
        tier: super::smart_router::ComplexityTier,
    ) -> Option<&str> {
        use super::smart_router::ComplexityTier;
        match tier {
            ComplexityTier::Low => self.haiku_model.as_deref(),
            ComplexityTier::Medium => self
                .sonnet_model
                .as_deref()
                .or(self.default_model.as_deref()),
            ComplexityTier::High => self.opus_model.as_deref().or(self.default_model.as_deref()),
        }
    }

    /// 根据原始模型名称获取映射后的模型（传统方式）
    pub fn map_model(&self, original_model: &str, has_thinking: bool) -> String {
        let model_lower = original_model.to_lowercase();

        // 1. thinking 模式优先使用推理模型
        if has_thinking {
            if let Some(ref m) = self.reasoning_model {
                return m.clone();
            }
        }

        // 2. 按模型类型匹配
        if model_lower.contains("haiku") {
            if let Some(ref m) = self.haiku_model {
                return m.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref m) = self.sonnet_model {
                return m.clone();
            }
        }

        // 3. 默认模型
        if let Some(ref m) = self.default_model {
            return m.clone();
        }

        // 4. 无映射，保持原样
        original_model.to_string()
    }
}

/// 检测请求是否启用了 thinking 模式
pub fn has_thinking_enabled(body: &Value) -> bool {
    match body
        .get("thinking")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("type"))
        .and_then(|t| t.as_str())
    {
        Some("enabled") | Some("adaptive") => true,
        Some("disabled") | None => false,
        Some(other) => {
            log::warn!(
                "[ModelMapper] 未知 thinking.type='{other}'，按 disabled 处理以避免误路由 reasoning 模型"
            );
            false
        }
    }
}

/// 对请求体应用模型映射（支持智能路由）
///
/// 返回 (映射后的请求体, 原始模型名, 映射后模型名)
///
/// # 智能路由逻辑
/// 1. 如果配置了层级模型（HAIKU/SONNET/OPUS），启用智能路由
/// 2. 分析请求复杂度，决定使用哪个层级
/// 3. 映射到配置的对应模型
/// 4. 如果没有配置层级模型，使用传统的模型名称匹配
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 如果没有配置映射，直接返回
    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 提取原始模型名
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    let Some(original) = original_model.as_deref() else {
        return (body, None, None);
    };

    let has_thinking = has_thinking_enabled(&body);

    // === 智能路由 ===
    // 如果配置了层级模型，根据请求复杂度选择模型
    if mapping.has_tier_models() {
        use super::smart_router::route;

        // 获取智能路由配置（可从 Provider meta 中读取）
        let config = get_smart_router_config(provider);

        // 执行智能路由
        let decision = route(&body, &config);

        // 获取目标模型
        let target_model = mapping.get_model_for_tier(decision.tier);

        if let Some(model) = target_model {
            log::info!(
                "[SmartRouter] {} → {} (tier: {:?}, confidence: {:.1}%, reasons: {})",
                original,
                model,
                decision.tier,
                decision.confidence * 100.0,
                decision.reasons.join(", ")
            );

            if model != original {
                body["model"] = serde_json::json!(model);
                return (body, Some(original.to_string()), Some(model.to_string()));
            } else {
                return (body, Some(original.to_string()), None);
            }
        }
    }

    // === 传统映射 ===
    // 根据模型名称中的关键词映射
    let mapped = mapping.map_model(original, has_thinking);

    log::info!(
        "[ModelMapper] 请求模型: {}, thinking={}, 映射结果: {}",
        original,
        has_thinking,
        mapped
    );

    if mapped != original {
        body["model"] = serde_json::json!(mapped);
        return (body, Some(original.to_string()), Some(mapped));
    }

    (body, Some(original.to_string()), None)
}

/// 从 Provider 配置获取智能路由配置
fn get_smart_router_config(_provider: &Provider) -> super::smart_router::SmartRouterConfig {
    // TODO: 从数据库或 Provider 配置读取智能路由配置
    // 目前使用默认配置
    super::smart_router::SmartRouterConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider_with_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped",
                    "ANTHROPIC_REASONING_MODEL": "reasoning-model"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_without_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_with_reasoning_only() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_REASONING_MODEL": "reasoning-only-model"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_sonnet_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-sonnet-4-5-20250929"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(original, Some("claude-sonnet-4-5-20250929".to_string()));
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_haiku_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-haiku-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_opus_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-opus-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_thinking_mode() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "reasoning-model");
        assert_eq!(mapped, Some("reasoning-model".to_string()));
    }

    #[test]
    fn test_reasoning_only_mapping_in_thinking_mode() {
        let provider = create_provider_with_reasoning_only();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "reasoning-only-model");
        assert_eq!(mapped, Some("reasoning-only-model".to_string()));
    }

    #[test]
    fn test_reasoning_only_mapping_does_not_affect_non_thinking() {
        let provider = create_provider_with_reasoning_only();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "disabled"}
        });
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "claude-sonnet-4-5");
        assert_eq!(original, Some("claude-sonnet-4-5".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_thinking_disabled() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "disabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_unknown_model_uses_default() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "some-unknown-model"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_no_mapping_configured() {
        let provider = create_provider_without_mapping();
        let body = json!({"model": "claude-sonnet-4-5"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "claude-sonnet-4-5");
        assert_eq!(original, Some("claude-sonnet-4-5".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_thinking_adaptive() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "adaptive"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "reasoning-model");
        assert_eq!(mapped, Some("reasoning-model".to_string()));
    }

    #[test]
    fn test_thinking_unknown_type() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "some_future_type"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_case_insensitive() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "Claude-SONNET-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_smart_routing_simple_query() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-opus-4-6",
            "messages": [
                {"role": "user", "content": "查找文件位置"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 简单查询应该路由到 haiku
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_smart_routing_complex_task() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "请重构这个架构，修复安全漏洞"},
            ],
            "thinking": {"type": "enabled"}
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 复杂任务应该路由到 opus（或 thinking 模式用 reasoning）
        assert!(result["model"] == "opus-mapped" || result["model"] == "reasoning-model");
    }
}
