//! 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换请求中的模型名称
//! 支持混合策略智能路由：综合原始模型层级 + 请求复杂度选择合适的模型

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
    /// 检查 provider 是否直接支持指定模型
    ///
    /// 这用于跳过不必要的智能路由：
    /// - 如果用户请求的模型是 provider 配置中的模型之一，直接使用
    /// - 避免将 "glm-5-turbo" 路由到其他模型
    pub fn has_model(&self, model: &str) -> bool {
        self.haiku_model.as_deref() == Some(model)
            || self.sonnet_model.as_deref() == Some(model)
            || self.opus_model.as_deref() == Some(model)
            || self.default_model.as_deref() == Some(model)
            || self.reasoning_model.as_deref() == Some(model)
    }

    /// 检查是否应该使用智能路由
    ///
    /// 智能路由启用的条件：
    /// 1. 配置了至少一个层级模型，或
    /// 2. 配置了 default_model（可以作为所有层级的fallback）
    ///
    /// 这确保了即使只配置了 default_model，也能根据请求复杂度进行路由决策
    pub fn should_use_smart_routing(&self) -> bool {
        self.has_tier_models() || self.default_model.is_some()
    }

    /// 获取指定层级的模型，支持 fallback 到 default_model
    ///
    /// Fallback 策略：
    /// - Low (haiku)  → haiku_model → default_model
    /// - Medium (sonnet) → sonnet_model → default_model
    /// - High (opus) → opus_model → default_model
    pub fn get_model_for_tier_with_fallback(
        &self,
        tier: super::smart_router::ComplexityTier,
    ) -> Option<&str> {
        // 首先尝试获取层级特定的模型
        if let Some(model) = self.get_model_for_tier(tier) {
            return Some(model);
        }

        // 如果没有层级特定模型，fallback 到 default_model
        self.default_model.as_deref()
    }

    /// 检查指定层级是否有专门的模型配置
    pub fn has_tier_model(&self, tier: super::smart_router::ComplexityTier) -> bool {
        match tier {
            super::smart_router::ComplexityTier::Low => self.haiku_model.is_some(),
            super::smart_router::ComplexityTier::Medium => self.sonnet_model.is_some(),
            super::smart_router::ComplexityTier::High => self.opus_model.is_some(),
        }
    }
}

/// 从原始模型名称中提取层级信息
///
/// 支持识别：
/// - haiku: claude-haiku-*, claude-3-5-haiku-*
/// - sonnet: claude-sonnet-*, claude-3-5-sonnet-*, claude-3-sonnet-*
/// - opus: claude-opus-*, claude-3-opus-*
fn extract_tier_from_model(model_name: &str) -> Option<super::smart_router::ComplexityTier> {
    use super::smart_router::ComplexityTier;

    let model_lower = model_name.to_lowercase();

    // 按优先级匹配（opus 需要在 sonnet 之前检查，因为 opus 可能包含 sonnet 的拼写）
    if model_lower.contains("opus") {
        return Some(ComplexityTier::High);
    }
    if model_lower.contains("sonnet") {
        return Some(ComplexityTier::Medium);
    }
    if model_lower.contains("haiku") {
        return Some(ComplexityTier::Low);
    }

    None
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

/// 对请求体应用模型映射（支持混合策略智能路由）
///
/// 返回 (映射后的请求体, 原始模型名, 映射后模型名)
///
/// # 混合策略路由逻辑
/// 1. 如果配置了层级模型（HAIKU/SONNET/OPUS），启用智能路由
/// 2. 提取原始模型层级作为基准
/// 3. SmartRouter 分析请求复杂度得到复杂度层级
/// 4. 综合两者决定最终层级（最多升级/降级配置的步数）
/// 5. 映射到配置的对应模型
/// 6. 如果没有配置层级模型，使用传统的模型名称匹配
///
/// # 混合策略示例（max_upgrade_steps=1, max_downgrade_steps=1）
/// - 原始 haiku + SmartRouter High   → sonnet (升级一级，不跨两级)
/// - 原始 sonnet + SmartRouter Low   → haiku (降一级)
/// - 原始 opus + SmartRouter Low     → sonnet (降一级)
/// - 原始 opus + SmartRouter High    → opus (保持)
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 提取原始模型名
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    let Some(original) = original_model.as_deref() else {
        return (body, None, None);
    };

    let has_thinking = has_thinking_enabled(&body);

    // === 特殊处理：thinking 模式优先使用 reasoning_model ===
    // reasoning_model 是专门用于复杂推理的模型，优先级高于智能路由
    if has_thinking {
        if let Some(reasoning_model) = &mapping.reasoning_model {
            log::info!(
                "[ModelMapper] Thinking 模式使用 reasoning_model: {}",
                reasoning_model
            );
            if reasoning_model != original {
                body["model"] = serde_json::json!(reasoning_model);
                return (body, Some(original.to_string()), Some(reasoning_model.clone()));
            }
            return (body, Some(original.to_string()), None);
        }
    }

    // === 特殊处理：检查 provider 是否直接支持请求的模型 ===
    // 如果请求的模型是 provider 配置中的模型之一，直接使用，不经过智能路由
    let provider_has_model = mapping.has_model(original);

    if provider_has_model {
        log::debug!(
            "[ModelMapper] Provider '{}' 直接支持请求的模型 '{}', 跳过智能路由",
            provider.name, original
        );
        return (body, Some(original.to_string()), None);
    }

    // === 智能路由优先路径 ===
    // 如果配置了层级模型（haiku/sonnet/opus），启用智能路由
    // 即使是第三方供应商（如智谱AI、OneAPI），只要配置了层级模型，也应该使用智能路由
    if mapping.has_tier_models() {
        use super::smart_router::route;

        // 获取智能路由配置
        let config = get_smart_router_config(provider);

        // 执行智能路由，获取复杂度层级
        let decision = route(&body, &config);

        // 提取原始模型层级
        let original_tier = extract_tier_from_model(original);

        // 计算最终层级
        let final_tier = if config.hybrid_mode {
            if let Some(orig_tier) = original_tier {
                combine_tiers(orig_tier, decision.tier, &config)
            } else {
                // 原始模型名称不含层级关键词，直接使用 SmartRouter 结果
                decision.tier
            }
        } else {
            // 非混合模式，直接使用 SmartRouter 结果
            decision.tier
        };

        // 获取目标模型（直接使用 get_model_for_tier，因为已经确认有 tier models）
        let target_model = mapping.get_model_for_tier(final_tier);

        if let Some(model) = target_model {
            let tier_change_info = if let Some(orig_tier) = original_tier {
                if orig_tier != final_tier {
                    format!(" ({} → {:?})", format_tier(orig_tier), final_tier)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let rule_info = decision
                .matched_rule
                .as_ref()
                .map(|r| format!(", rule: {}", r))
                .unwrap_or_default();

            log::info!(
                "[SmartRouter] {} → {} (original: {:?}, router: {:?}, final: {:?}{}{}, confidence: {:.1}%, reasons: {})",
                original,
                model,
                original_tier,
                decision.tier,
                final_tier,
                tier_change_info,
                rule_info,
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

    // === 传统映射路径 ===
    // 没有配置层级模型，使用传统模型名称映射

    // 如果没有配置任何映射，直接返回
    if !mapping.has_mapping() {
        return (body, Some(original.to_string()), None);
    }

    // 对于没有配置层级模型的非 Claude 供应商，使用传统映射
    let is_non_claude = is_non_claude_provider(provider, original);
    if is_non_claude {
        let mapped = mapping.map_model(original, has_thinking);
        log::info!(
            "[ModelMapper] 传统映射路径（非Claude供应商）: {} → {}",
            original,
            mapped
        );
        if mapped != original {
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.to_string()), Some(mapped));
        }
        return (body, Some(original.to_string()), None);
    }

    // === 混合策略智能路由（default_model 作为 fallback） ===
    // 如果配置了 default_model 但没有层级模型，使用智能路由 + default_model fallback
    if mapping.should_use_smart_routing() {
        use super::smart_router::route;

        // 获取智能路由配置
        let config = get_smart_router_config(provider);

        // 执行智能路由，获取复杂度层级
        let decision = route(&body, &config);

        // 提取原始模型层级
        let original_tier = extract_tier_from_model(original);

        // 计算最终层级
        let final_tier = if config.hybrid_mode {
            if let Some(orig_tier) = original_tier {
                combine_tiers(orig_tier, decision.tier, &config)
            } else {
                // 原始模型名称不含层级关键词，直接使用 SmartRouter 结果
                decision.tier
            }
        } else {
            // 非混合模式，直接使用 SmartRouter 结果
            decision.tier
        };

        // 使用新的带 fallback 的方法获取目标模型
        let target_model = mapping.get_model_for_tier_with_fallback(final_tier);

        if let Some(model) = target_model {
            let tier_change_info = if let Some(orig_tier) = original_tier {
                if orig_tier != final_tier {
                    format!(" ({} → {:?})", format_tier(orig_tier), final_tier)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let rule_info = decision
                .matched_rule
                .as_ref()
                .map(|r| format!(", rule: {}", r))
                .unwrap_or_default();

            let using_fallback = mapping.has_tier_model(final_tier);
            let fallback_info = if !using_fallback {
                " (using default_model)"
            } else {
                ""
            };

            log::info!(
                "[SmartRouter] {} → {} (original: {:?}, router: {:?}, final: {:?}{}{}{}, confidence: {:.1}%, reasons: {})",
                original,
                model,
                original_tier,
                decision.tier,
                final_tier,
                tier_change_info,
                fallback_info,
                rule_info,
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

/// 综合原始层级和 SmartRouter 层级，计算最终层级
///
/// 规则：
/// - 最多升级/降级配置的步数
/// - opus + never_downgrade_opus 配置时，opus 不降级
fn combine_tiers(
    original: super::smart_router::ComplexityTier,
    router: super::smart_router::ComplexityTier,
    config: &super::smart_router::SmartRouterConfig,
) -> super::smart_router::ComplexityTier {
    use super::smart_router::ComplexityTier;

    // 层级数值化：Low=0, Medium=1, High=2
    let tier_value = |t: ComplexityTier| match t {
        ComplexityTier::Low => 0,
        ComplexityTier::Medium => 1,
        ComplexityTier::High => 2,
    };

    let value_to_tier = |v: i32| match v {
        0 => ComplexityTier::Low,
        1 => ComplexityTier::Medium,
        _ => ComplexityTier::High,
    };

    let orig_val = tier_value(original);
    let router_val = tier_value(router);

    // 计算差值（正数 = 需要升级，负数 = 需要降级）
    let diff = router_val - orig_val;

    let final_val = if diff > 0 {
        // 需要升级
        let max_upgrade = config.max_upgrade_steps as i32;
        orig_val + diff.min(max_upgrade)
    } else if diff < 0 {
        // 需要降级
        // 检查是否禁止 opus 降级
        if original == ComplexityTier::High && config.never_downgrade_opus {
            orig_val // 保持原层级
        } else {
            let max_downgrade = config.max_downgrade_steps as i32;
            orig_val + diff.max(-max_downgrade) // diff 是负数
        }
    } else {
        // 层级相同，保持
        orig_val
    };

    // 应用 min/max 限制
    let final_tier = value_to_tier(final_val);

    let final_tier = match config.min_tier {
        Some(min) if final_tier < min => min,
        _ => final_tier,
    };

    match config.max_tier {
        Some(max) if final_tier > max => max,
        _ => final_tier,
    }
}

/// 格式化层级名称用于日志
fn format_tier(tier: super::smart_router::ComplexityTier) -> &'static str {
    use super::smart_router::ComplexityTier;
    match tier {
        ComplexityTier::Low => "Low",
        ComplexityTier::Medium => "Medium",
        ComplexityTier::High => "High",
    }
}

/// 从 Provider 配置获取智能路由配置
///
/// 配置读取顺序（从高到低）：
/// 1. Provider.settings_config.smartRouter - 供应商级别的智能路由配置
/// 2. Provider.meta - 用户自定义配置（未来扩展）
/// 3. 默认配置
///
/// 配置格式示例（在 settings_config 中）：
/// ```json
/// {
///   "smartRouter": {
///     "enabled": true,
///     "low_threshold": 4,
///     "high_threshold": 8,
///     "hybrid_mode": true,
///     "max_upgrade_steps": 1,
///     "max_downgrade_steps": 1,
///     "never_downgrade_opus": false,
///     "min_tier": "low",
///     "max_tier": "high"
///   }
/// }
/// ```
fn get_smart_router_config(provider: &Provider) -> super::smart_router::SmartRouterConfig {
    use super::smart_router::SmartRouterConfig;

    // 尝试从 Provider.settings_config 读取智能路由配置
    if let Some(router_config) = provider.settings_config.get("smartRouter") {
        if let Ok(config) = serde_json::from_value::<SmartRouterConfig>(router_config.clone()) {
            log::debug!("[SmartRouter] 使用供应商级别的智能路由配置");
            return config;
        } else {
            log::warn!("[SmartRouter] 供应商的 smartRouter 配置格式无效，使用默认配置");
        }
    }

    // 尝试从 env.SMART_ROUTER_CONFIG 读取（环境变量方式，支持 JSON）
    if let Some(env) = provider.settings_config.get("env") {
        if let Some(config_str) = env.get("SMART_ROUTER_CONFIG").and_then(|v| v.as_str()) {
            if let Ok(config_value) = serde_json::from_str::<serde_json::Value>(config_str) {
                if let Ok(config) = serde_json::from_value::<SmartRouterConfig>(config_value) {
                    log::debug!("[SmartRouter] 使用环境变量配置的智能路由");
                    return config;
                }
            }
        }
    }

    // 使用默认配置
    log::debug!("[SmartRouter] 使用默认智能路由配置");
    SmartRouterConfig::default()
}

/// 检查是否使用了非 Claude 供应商
///
/// 非 Claude 供应商包括：
/// - 自定义 ANTHROPIC_BASE_URL 指向非 anthropic.com
/// - 非 Claude 模型 ID（模型名称不包含 "claude"）
///
/// **重要**: 此函数仅在未配置层级模型时使用。
/// 如果供应商配置了 haiku/sonnet/opus 层级模型（如智谱AI配置了 glm-4.7-flashx/4.7/5-turbo），
/// 则无论是否为"原生" Claude 供应商，都应该使用智能路由。
///
/// 此函数用于判断是否跳过智能路由，只在没有配置层级模型时生效。
pub fn is_non_claude_provider(provider: &Provider, original_model: &str) -> bool {
    // 检查自定义 base_url
    if let Some(env) = provider.settings_config.get("env") {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
            // 如果 base_url 不指向 anthropic.com，视为非 Claude 供应商
            if !base_url.is_empty() && !base_url.contains("anthropic.com") {
                log::debug!("[ModelMapper] 检测到自定义 BASE_URL，视为非 Claude 供应商");
                return true;
            }
        }
    }

    // 检查模型名称是否包含 "claude"
    let model_lower = original_model.to_lowercase();
    if !model_lower.contains("claude") {
        log::debug!("[ModelMapper] 模型名称不包含 'claude'，视为非 Claude 供应商: {}", original_model);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
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
    fn test_smart_routing_simple_query_with_opus() {
        // 混合策略：原始 opus + SmartRouter Low = sonnet (降一级)
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-opus-4-6",
            "messages": [
                {"role": "user", "content": "查找文件位置"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 原始 opus + 简单查询 → 混合策略降一级到 sonnet
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_smart_routing_simple_query_with_haiku() {
        // 混合策略：原始 haiku + SmartRouter Low = haiku (保持)
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "查找文件位置"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 原始 haiku + 简单查询 → 保持 haiku
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_smart_routing_complex_with_haiku() {
        // 混合策略：原始 haiku + SmartRouter High = sonnet (升一级，不跨两级)
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "请重构整个项目的架构，修复安全漏洞"},
            ],
            "thinking": {"type": "enabled"}
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 原始 haiku + 复杂任务 → 升一级到 sonnet（或 thinking 用 reasoning）
        assert!(
            result["model"] == "sonnet-mapped" || result["model"] == "reasoning-model",
            "Expected sonnet-mapped or reasoning-model, got {}",
            result["model"]
        );
        assert!(mapped.is_some());
    }

    #[test]
    fn test_smart_routing_complex_with_sonnet() {
        // 混合策略：原始 sonnet + SmartRouter High = opus (升一级)
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "请重构整个项目的架构，修复安全漏洞"},
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 原始 sonnet + 复杂任务 → 升一级到 opus
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_smart_routing_complex_with_opus() {
        // 混合策略：原始 opus + SmartRouter High = opus (保持)
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-opus-4-6",
            "messages": [
                {"role": "user", "content": "请重构整个项目的架构，修复安全漏洞"},
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 原始 opus + 复杂任务 → 保持 opus
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_extract_tier_from_model() {
        use super::super::smart_router::ComplexityTier;

        assert_eq!(extract_tier_from_model("claude-haiku-4-5"), Some(ComplexityTier::Low));
        assert_eq!(extract_tier_from_model("claude-3-5-haiku-20241022"), Some(ComplexityTier::Low));
        assert_eq!(extract_tier_from_model("Claude-HAIKU-4-5"), Some(ComplexityTier::Low));

        assert_eq!(extract_tier_from_model("claude-sonnet-4-5"), Some(ComplexityTier::Medium));
        assert_eq!(extract_tier_from_model("claude-3-5-sonnet-20241022"), Some(ComplexityTier::Medium));

        assert_eq!(extract_tier_from_model("claude-opus-4-6"), Some(ComplexityTier::High));
        assert_eq!(extract_tier_from_model("claude-3-opus-20240229"), Some(ComplexityTier::High));

        // 不含层级关键词
        assert_eq!(extract_tier_from_model("gpt-4"), None);
        assert_eq!(extract_tier_from_model("deepseek-v3"), None);
    }

    // =========================================================================
    // 非Claude供应商测试（智谱AI、OneAPI等）
    // =========================================================================

    /// 创建智谱AI供应商配置（用户实际使用的配置）
    fn create_zhipuai_provider() -> Provider {
        use crate::provider::ProviderMeta;
        use std::collections::HashMap;

        Provider {
            id: "95da9505-69b5-4fb4-a402-30a8a35f8f8a".to_string(),
            name: "zhipuai-mix".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "94b6ffa499af42ac82c84025249fa7e2.zDZkLYUgcyVXuI5P",
                    "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7-flashx",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5-turbo",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-4.7",
                    "ANTHROPIC_MODEL": "glm-4.7",
                    "ANTHROPIC_REASONING_MODEL": "glm-5-turbo"
                }
            }),
            website_url: Some("https://open.bigmodel.cn".to_string()),
            category: Some("cn_official".to_string()),
            created_at: Some(1772791904787),
            sort_index: Some(0),
            notes: None,
            meta: Some(ProviderMeta {
                custom_endpoints: HashMap::new(),
                common_config_enabled: Some(true),
                usage_script: None,
                endpoint_auto_select: Some(true),
                is_partner: None,
                partner_promotion_key: None,
                cost_multiplier: None,
                pricing_model_source: None,
                limit_daily_usd: None,
                limit_monthly_usd: None,
                test_config: None,
                proxy_config: None,
                api_format: Some("anthropic".to_string()),
                api_key_field: None,
                prompt_cache_key: None,
            }),
            icon: Some("zhipu".to_string()),
            icon_color: Some("#0F62FE".to_string()),
            in_failover_queue: false,
        }
    }

    /// 创建没有层级模型的非Claude供应商（应该使用传统映射）
    fn create_non_claude_provider_without_tier_models() -> Provider {
        Provider {
            id: "oneapi".to_string(),
            name: "OneAPI".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://oneapi.example.com/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                    "ANTHROPIC_MODEL": "gpt-4o"
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
    fn test_zhipuai_provider_has_tier_models() {
        let provider = create_zhipuai_provider();
        let mapping = ModelMapping::from_provider(&provider);

        // 验证配置被正确解析
        assert_eq!(mapping.haiku_model, Some("glm-4.7-flashx".to_string()));
        assert_eq!(mapping.sonnet_model, Some("glm-4.7".to_string()));
        assert_eq!(mapping.opus_model, Some("glm-5-turbo".to_string()));
        assert_eq!(mapping.default_model, Some("glm-4.7".to_string()));
        assert_eq!(mapping.reasoning_model, Some("glm-5-turbo".to_string()));

        // has_tier_models 应该返回 true
        assert!(mapping.has_tier_models());
    }

    #[test]
    fn test_zhipuai_simple_query_routes_to_flashx() {
        // 简单查询应该路由到 haiku (glm-4.7-flashx)
        let provider = create_zhipuai_provider();
        let body = json!({
            "model": "claude-haiku-4-5",
            "messages": [
                {"role": "user", "content": "查找文件位置"}
            ]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(original, Some("claude-haiku-4-5".to_string()));
        // 简单查询，原始 haiku → 保持 haiku 层级
        assert_eq!(result["model"], "glm-4.7-flashx");
        assert_eq!(mapped, Some("glm-4.7-flashx".to_string()));
    }

    #[test]
    fn test_zhipuai_complex_query_routes_to_turbo() {
        // 复杂查询应该路由到 opus (glm-5-turbo)
        let provider = create_zhipuai_provider();
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "请重构整个项目的架构，修复安全漏洞"}
            ]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(original, Some("claude-sonnet-4-6".to_string()));
        // 复杂查询，原始 sonnet + SmartRouter High → 升级到 opus
        assert_eq!(result["model"], "glm-5-turbo");
        assert_eq!(mapped, Some("glm-5-turbo".to_string()));
    }

    #[test]
    fn test_zhipuai_medium_query_stays_on_medium() {
        // 中等复杂度查询保持 sonnet 层级
        let provider = create_zhipuai_provider();
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "帮我分析一下这个函数的逻辑"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 中等复杂度，保持 sonnet 层级
        assert_eq!(result["model"], "glm-4.7");
        assert_eq!(mapped, Some("glm-4.7".to_string()));
    }

    #[test]
    fn test_zhipuai_opus_with_simple_query_downgrades_to_sonnet() {
        // opus + 简单查询 → 降级到 sonnet
        let provider = create_zhipuai_provider();
        let body = json!({
            "model": "claude-opus-4-6",
            "messages": [
                {"role": "user", "content": "显示代码"}
            ]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(original, Some("claude-opus-4-6".to_string()));
        // 原始 opus + 简单查询 → 降级到 sonnet (glm-4.7)
        assert_eq!(result["model"], "glm-4.7");
        assert_eq!(mapped, Some("glm-4.7".to_string()));
    }

    #[test]
    fn test_non_claude_provider_without_tier_models_uses_traditional_mapping() {
        // 没有层级模型的非Claude供应商使用传统映射
        let provider = create_non_claude_provider_without_tier_models();
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "简单查询"}
            ]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);
        // 没有配置层级模型，使用传统映射（gpt-4o）
        assert_eq!(result["model"], "gpt-4o");
        assert_eq!(original, Some("claude-sonnet-4-6".to_string()));
        assert_eq!(mapped, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_provider_with_smart_router_config() {
        // 测试从 settings_config.smartRouter 读取配置
        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
                },
                "smartRouter": {
                    "enabled": true,
                    "low_threshold": 3,
                    "high_threshold": 10,
                    "hybrid_mode": true,
                    "max_upgrade_steps": 2,
                    "max_downgrade_steps": 2
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
        };

        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "非常复杂的任务，需要重构整个系统架构"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // 复杂任务应该升级到 opus（因为 max_upgrade_steps=2）
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_provider_with_only_default_model_uses_smart_routing() {
        // 只配置了 default_model，没有层级模型，应该使用智能路由 + default_model fallback
        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "single-model-for-all"
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
        };

        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "查找文件"}
            ]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);
        // 没有配置层级模型但有 default_model，所有请求都使用 default_model
        // 但智能路由仍然会分析复杂度（只是映射结果相同）
        assert_eq!(result["model"], "single-model-for-all");
        assert_eq!(original, Some("claude-sonnet-4-6".to_string()));
        assert_eq!(mapped, Some("single-model-for-all".to_string()));
    }

    #[test]
    fn test_should_use_smart_routing_with_tier_models() {
        let provider = create_zhipuai_provider();
        let mapping = ModelMapping::from_provider(&provider);
        // 配置了层级模型，应该启用智能路由
        assert!(mapping.should_use_smart_routing());
    }

    #[test]
    fn test_should_use_smart_routing_with_only_default_model() {
        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model"
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
        };
        let mapping = ModelMapping::from_provider(&provider);
        // 只有 default_model，也应该启用智能路由（fallback 行为）
        assert!(mapping.should_use_smart_routing());
    }

    #[test]
    fn test_should_use_smart_routing_without_any_config() {
        let provider = create_provider_without_mapping();
        let mapping = ModelMapping::from_provider(&provider);
        // 没有任何配置，不启用智能路由
        assert!(!mapping.should_use_smart_routing());
    }

    #[test]
    fn test_get_model_for_tier_with_fallback() {
        use crate::proxy::smart_router::ComplexityTier;

        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-fallback",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-specific"
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
        };
        let mapping = ModelMapping::from_provider(&provider);

        // Medium 层级有专门配置
        assert_eq!(
            mapping.get_model_for_tier_with_fallback(ComplexityTier::Medium),
            Some("sonnet-specific")
        );

        // Low 层级没有专门配置，应该 fallback 到 default_model
        assert_eq!(
            mapping.get_model_for_tier_with_fallback(ComplexityTier::Low),
            Some("default-fallback")
        );

        // High 层级没有专门配置，应该 fallback 到 default_model
        assert_eq!(
            mapping.get_model_for_tier_with_fallback(ComplexityTier::High),
            Some("default-fallback")
        );
    }

    #[test]
    fn test_zhipuai_thinking_mode_uses_reasoning_model() {
        let provider = create_zhipuai_provider();
        let body = json!({
            "model": "claude-sonnet-4-6",
            "thinking": {"type": "enabled"},
            "messages": [
                {"role": "user", "content": "普通问题"}
            ]
        });

        let (result, _, mapped) = apply_model_mapping(body, &provider);
        // thinking 模式优先使用 reasoning_model (glm-5-turbo)
        assert_eq!(result["model"], "glm-5-turbo");
        assert_eq!(mapped, Some("glm-5-turbo".to_string()));
    }

    // =========================================================================
    // 新增测试：验证 has_model() 和跳过智能路由的逻辑
    // =========================================================================

    #[test]
    fn test_has_model_detects_all_configured_models() {
        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-model",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-model",
                    "ANTHROPIC_MODEL": "default-model",
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
        };
        let mapping = ModelMapping::from_provider(&provider);

        // 所有配置的模型都应该被识别
        assert!(mapping.has_model("haiku-model"));
        assert!(mapping.has_model("sonnet-model"));
        assert!(mapping.has_model("opus-model"));
        assert!(mapping.has_model("default-model"));
        assert!(mapping.has_model("reasoning-model"));

        // 未配置的模型不应该被识别
        assert!(!mapping.has_model("unknown-model"));
        assert!(!mapping.has_model("glm-5-turbo"));
    }

    #[test]
    fn test_provider_directly_supports_requested_model_skip_smart_routing() {
        // 模拟 baidu-mix 场景：provider 配置了 glm-5，用户请求 glm-5
        let provider = Provider {
            id: "baidu-mix".to_string(),
            name: "baidu-mix".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "minimax-m2.5",
                    "ANTHROPIC_MODEL": "deepseek-v3.2",
                    "ANTHROPIC_REASONING_MODEL": "glm-5"
                }
            }),
            website_url: Some("https://cloud.baidu.com/".to_string()),
            category: None,
            created_at: Some(1773237487448),
            sort_index: Some(1),
            notes: None,
            meta: Some(serde_json::from_str(r#"{"commonConfigEnabled":true,"endpointAutoSelect":true,"apiFormat":"anthropic"}"#).unwrap()),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        // 用户请求 glm-5，这是 provider 直接支持的模型
        let body = json!({
            "model": "glm-5",
            "messages": [{"role": "user", "content": "测试"}]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);

        // 应该直接使用 glm-5，不经过智能路由
        assert_eq!(result["model"], "glm-5");
        assert_eq!(original, Some("glm-5".to_string()));
        assert_eq!(mapped, None); // 没有映射，直接使用原模型
    }

    #[test]
    fn test_thinking_mode_uses_reasoning_model_when_available() {
        // baidu-mix 场景：thinking 模式应该使用 reasoning_model (glm-5)
        let provider = Provider {
            id: "baidu-mix".to_string(),
            name: "baidu-mix".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "minimax-m2.5",
                    "ANTHROPIC_MODEL": "deepseek-v3.2",
                    "ANTHROPIC_REASONING_MODEL": "glm-5"
                }
            }),
            website_url: Some("https://cloud.baidu.com/".to_string()),
            category: None,
            created_at: Some(1773237487448),
            sort_index: Some(1),
            notes: None,
            meta: Some(serde_json::from_str(r#"{"commonConfigEnabled":true,"endpointAutoSelect":true,"apiFormat":"anthropic"}"#).unwrap()),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        // thinking 模式启用，请求的是其他模型
        let body = json!({
            "model": "minimax-m2.5",
            "thinking": {"type": "enabled"},
            "messages": [{"role": "user", "content": "复杂推理问题"}]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);

        // thinking 模式应该使用 reasoning_model (glm-5)
        assert_eq!(result["model"], "glm-5");
        assert_eq!(original, Some("minimax-m2.5".to_string()));
        assert_eq!(mapped, Some("glm-5".to_string()));
    }

    #[test]
    fn test_unsupported_model_passes_through_when_no_smart_routing() {
        // provider 没有配置层级模型，也不支持请求的模型
        let provider = Provider {
            id: "simple".to_string(),
            name: "Simple".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model"
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
        };
        let mapping = ModelMapping::from_provider(&provider);

        // 没有层级模型，不应该启用智能路由
        assert!(!mapping.has_tier_models());
        assert!(!mapping.has_model("requested-model"));

        // 请求一个不存在的模型
        let body = json!({
            "model": "requested-model",
            "messages": [{"role": "user", "content": "test"}]
        });

        let (result, original, mapped) = apply_model_mapping(body, &provider);

        // 应该保持原模型不变（因为没有映射配置）
        assert_eq!(result["model"], "requested-model");
        assert_eq!(original, Some("requested-model".to_string()));
        assert_eq!(mapped, None);
    }

    #[test]
    fn test_baidu_mix_does_not_support_glm_5_turbo() {
        // 验证 baidu-mix 不支持 glm-5-turbo
        let provider = Provider {
            id: "baidu-mix".to_string(),
            name: "baidu-mix".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "minimax-m2.5",
                    "ANTHROPIC_MODEL": "deepseek-v3.2",
                    "ANTHROPIC_REASONING_MODEL": "glm-5"
                }
            }),
            website_url: Some("https://cloud.baidu.com/".to_string()),
            category: None,
            created_at: Some(1773237487448),
            sort_index: Some(1),
            notes: None,
            meta: Some(serde_json::from_str(r#"{"commonConfigEnabled":true,"endpointAutoSelect":true,"apiFormat":"anthropic"}"#).unwrap()),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        let mapping = ModelMapping::from_provider(&provider);

        // baidu-mix 不支持 glm-5-turbo
        assert!(!mapping.has_model("glm-5-turbo"));
        // baidu-mix 支持 glm-5
        assert!(mapping.has_model("glm-5"));
    }
}
