//! 智能模型路由模块
//!
//! 根据请求复杂度自动选择合适的模型层级（Haiku/Sonnet/Opus）
//! 参考 oh-my-claudecode 的智能路由实现

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 模型复杂度层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComplexityTier {
    /// 低复杂度 - 使用 Haiku
    Low,
    /// 中等复杂度 - 使用 Sonnet
    Medium,
    /// 高复杂度 - 使用 Opus
    High,
}

impl Default for ComplexityTier {
    fn default() -> Self {
        Self::Medium
    }
}

impl ComplexityTier {
    /// 从原始模型名推断层级
    pub fn from_model_name(model: &str) -> Option<Self> {
        let model_lower = model.to_lowercase();
        if model_lower.contains("haiku") {
            Some(Self::Low)
        } else if model_lower.contains("opus") {
            Some(Self::High)
        } else if model_lower.contains("sonnet") {
            Some(Self::Medium)
        } else {
            None
        }
    }
}

/// 复杂度信号
#[derive(Debug, Default)]
pub struct ComplexitySignals {
    // === 词汇信号 ===
    /// 消息总字数
    pub total_chars: usize,
    /// 文件路径数量
    pub file_path_count: usize,
    /// 代码块数量
    pub code_block_count: usize,
    /// 是否包含架构关键词
    pub has_architecture_keywords: bool,
    /// 是否包含调试关键词
    pub has_debugging_keywords: bool,
    /// 是否包含简单关键词
    pub has_simple_keywords: bool,
    /// 是否包含风险关键词
    pub has_risk_keywords: bool,

    // === 结构信号 ===
    /// 消息数量
    pub message_count: usize,
    /// 工具调用数量
    pub tool_count: usize,
    /// 是否启用 thinking
    pub has_thinking: bool,
    /// 系统提示长度
    pub system_prompt_chars: usize,

    // === 上下文信号 ===
    /// 是否有长对话历史
    pub has_long_history: bool,
}

/// 路由决策结果
#[derive(Debug)]
pub struct RoutingDecision {
    /// 目标层级
    pub tier: ComplexityTier,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 决策原因
    pub reasons: Vec<String>,
}

/// 智能路由器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRouterConfig {
    /// 是否启用智能路由
    pub enabled: bool,
    /// 最低层级（不允许低于此层级）
    pub min_tier: Option<ComplexityTier>,
    /// 最高层级（不允许高于此层级）
    pub max_tier: Option<ComplexityTier>,
    /// Low 层级阈值（分数 < 此值为 Low）
    pub low_threshold: i32,
    /// High 层级阈值（分数 >= 此值为 High）
    pub high_threshold: i32,
}

impl Default for SmartRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_tier: None,
            max_tier: None,
            low_threshold: 4,
            high_threshold: 8,
        }
    }
}

/// 关键词定义
mod keywords {
    pub const ARCHITECTURE: &[&str] = &[
        "architecture", "refactor", "redesign", "restructure", "migrate",
        "redesign", "design pattern", "system design", "架构", "重构",
        "迁移", "设计模式",
    ];

    pub const DEBUGGING: &[&str] = &[
        "debug", "diagnose", "root cause", "investigate", "trace",
        "fix bug", "error", "exception", "stack trace", "调试",
        "诊断", "排查", "修复", "错误",
    ];

    pub const SIMPLE: &[&str] = &[
        "find", "search", "locate", "list", "show", "where is",
        "what is", "get", "read", "查找", "搜索", "列出", "显示",
        "是什么", "读取",
    ];

    pub const RISK: &[&str] = &[
        "critical", "production", "urgent", "security", "breaking",
        "important", "key", "核心", "重要", "紧急", "安全",
    ];
}

/// 从请求体提取复杂度信号
pub fn extract_signals(body: &Value) -> ComplexitySignals {
    let mut signals = ComplexitySignals::default();

    // 提取消息
    let messages = body.get("messages").and_then(|m| m.as_array());

    if let Some(messages) = messages {
        signals.message_count = messages.len();
        signals.has_long_history = messages.len() > 10;

        for msg in messages {
            // 统计字数
            if let Some(content) = msg.get("content") {
                let text = extract_text_from_content(content);
                signals.total_chars += text.chars().count();

                // 统计文件路径
                signals.file_path_count += count_file_paths(&text);

                // 统计代码块
                signals.code_block_count += text.matches("```").count() / 2;

                // 检测关键词
                let text_lower = text.to_lowercase();
                signals.has_architecture_keywords |=
                    keywords::ARCHITECTURE.iter().any(|k| text_lower.contains(k));
                signals.has_debugging_keywords |=
                    keywords::DEBUGGING.iter().any(|k| text_lower.contains(k));
                signals.has_simple_keywords |=
                    keywords::SIMPLE.iter().any(|k| text_lower.contains(k));
                signals.has_risk_keywords |=
                    keywords::RISK.iter().any(|k| text_lower.contains(k));
            }

            // 统计工具调用
            if let Some(content) = msg.get("content") {
                if let Some(content_arr) = content.as_array() {
                    for block in content_arr {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            signals.tool_count += 1;
                        }
                    }
                }
            }
        }
    }

    // 系统提示
    if let Some(system) = body.get("system") {
        let system_text = if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            String::new()
        };
        signals.system_prompt_chars = system_text.chars().count();
    }

    // thinking 模式
    signals.has_thinking = super::model_mapper::has_thinking_enabled(body);

    signals
}

/// 从 content 字段提取文本
fn extract_text_from_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            arr.iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => String::new(),
    }
}

/// 统计文件路径数量
fn count_file_paths(text: &str) -> usize {
    // 匹配常见文件路径模式
    let patterns = [
        // Windows 路径: C:\path\to\file.ext 或 C:/path/to/file.ext
        r"[A-Za-z]:[/\\][\w\-./\\]+\.\w{1,10}",
        // Unix 路径: /path/to/file.ext
        r"/[\w\-./]+\.\w{1,10}",
        // 相对路径: ./path/file.ext 或 ../path/file.ext
        r"\.{1,2}/[\w\-./]+\.\w{1,10}",
        // 文件名带扩展名（独立出现）
        r"\b[\w\-]+\.\w{1,10}\b",
    ];

    let mut count = 0;
    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            count += re.find_iter(text).count();
        }
    }

    // 去重（同一个路径可能被多次匹配）
    count.min(text.len() / 5) // 粗略上限
}

/// 计算复杂度分数
pub fn calculate_score(signals: &ComplexitySignals) -> i32 {
    let mut score = 0;

    // === 词汇信号权重 ===
    if signals.has_architecture_keywords {
        score += 3;
    }
    if signals.has_debugging_keywords {
        score += 2;
    }
    if signals.has_risk_keywords {
        score += 2;
    }
    if signals.has_simple_keywords {
        score -= 2;
    }

    // === 结构信号权重 ===
    // 消息数量
    if signals.message_count > 20 {
        score += 2;
    } else if signals.message_count > 10 {
        score += 1;
    }

    // 工具调用
    if signals.tool_count > 10 {
        score += 3;
    } else if signals.tool_count > 5 {
        score += 2;
    } else if signals.tool_count > 2 {
        score += 1;
    }

    // thinking 模式通常是复杂任务
    if signals.has_thinking {
        score += 2;
    }

    // 代码块数量
    if signals.code_block_count > 5 {
        score += 2;
    } else if signals.code_block_count > 2 {
        score += 1;
    }

    // 系统提示长度
    if signals.system_prompt_chars > 5000 {
        score += 2;
    } else if signals.system_prompt_chars > 2000 {
        score += 1;
    }

    // 长对话历史
    if signals.has_long_history {
        score += 1;
    }

    // === 文件路径暗示复杂度 ===
    if signals.file_path_count > 10 {
        score += 2;
    } else if signals.file_path_count > 5 {
        score += 1;
    }

    score
}

/// 根据分数决定层级
fn score_to_tier(score: i32, config: &SmartRouterConfig) -> ComplexityTier {
    let tier = if score >= config.high_threshold {
        ComplexityTier::High
    } else if score < config.low_threshold {
        ComplexityTier::Low
    } else {
        ComplexityTier::Medium
    };

    // 应用层级限制
    let tier = match config.min_tier {
        Some(min) if tier < min => min,
        _ => tier,
    };

    match config.max_tier {
        Some(max) if tier > max => max,
        _ => tier,
    }
}

/// 智能路由决策
pub fn route(body: &Value, config: &SmartRouterConfig) -> RoutingDecision {
    if !config.enabled {
        return RoutingDecision {
            tier: ComplexityTier::Medium,
            confidence: 1.0,
            reasons: vec!["智能路由已禁用".to_string()],
        };
    }

    // 提取信号
    let signals = extract_signals(body);

    // 计算分数
    let score = calculate_score(&signals);

    // 决定层级
    let tier = score_to_tier(score, config);

    // 收集原因
    let mut reasons = Vec::new();
    reasons.push(format!("复杂度分数: {}", score));

    if signals.has_architecture_keywords {
        reasons.push("包含架构关键词".to_string());
    }
    if signals.has_debugging_keywords {
        reasons.push("包含调试关键词".to_string());
    }
    if signals.has_risk_keywords {
        reasons.push("包含风险关键词".to_string());
    }
    if signals.has_simple_keywords {
        reasons.push("包含简单关键词".to_string());
    }
    if signals.tool_count > 5 {
        reasons.push(format!("工具调用较多 ({})", signals.tool_count));
    }
    if signals.has_thinking {
        reasons.push("启用 thinking 模式".to_string());
    }
    if signals.code_block_count > 3 {
        reasons.push(format!("代码块较多 ({})", signals.code_block_count));
    }

    // 计算置信度
    let confidence = if score < config.low_threshold || score >= config.high_threshold {
        0.9 // 边界情况置信度高
    } else {
        0.7 // 中间区域置信度中等
    };

    RoutingDecision {
        tier,
        confidence,
        reasons,
    }
}

/// 模型映射配置（从 Provider 的 env 中读取）
pub struct TierModelMapping {
    pub low_model: Option<String>,
    pub medium_model: Option<String>,
    pub high_model: Option<String>,
    pub default_model: Option<String>,
}

impl TierModelMapping {
    /// 从 Provider 配置提取层级模型映射
    pub fn from_provider_env(env: &serde_json::Map<String, Value>) -> Self {
        Self {
            low_model: env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            medium_model: env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            high_model: env
                .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .get("ANTHROPIC_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了智能路由所需的模型
    pub fn has_tier_models(&self) -> bool {
        self.low_model.is_some() || self.medium_model.is_some() || self.high_model.is_some()
    }

    /// 根据层级获取模型名
    pub fn get_model_for_tier(&self, tier: ComplexityTier) -> Option<&str> {
        match tier {
            ComplexityTier::Low => self.low_model.as_deref(),
            ComplexityTier::Medium => self.medium_model.as_deref().or(self.default_model.as_deref()),
            ComplexityTier::High => self.high_model.as_deref().or(self.default_model.as_deref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_signals_simple_query() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "查找文件 test.rs"}
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.has_simple_keywords);
        assert!(!signals.has_architecture_keywords);
    }

    #[test]
    fn test_extract_signals_architecture() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "请重构这个架构设计模式"}
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.has_architecture_keywords);
    }

    #[test]
    fn test_calculate_score_simple() {
        let signals = ComplexitySignals {
            has_simple_keywords: true,
            ..Default::default()
        };

        let score = calculate_score(&signals);
        assert!(score < 0);
    }

    #[test]
    fn test_calculate_score_complex() {
        let signals = ComplexitySignals {
            has_architecture_keywords: true,
            has_debugging_keywords: true,
            tool_count: 8,
            has_thinking: true,
            ..Default::default()
        };

        let score = calculate_score(&signals);
        assert!(score >= 8);
    }

    #[test]
    fn test_route_simple() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "查找文件位置"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::Low);
    }

    #[test]
    fn test_route_complex() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "请重构这个架构，修复安全漏洞"},
                {"role": "assistant", "content": "好的，我来分析..."},
                {"role": "user", "content": "继续调试这个错误"},
            ],
            "thinking": {"type": "enabled"}
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::High);
    }

    #[test]
    fn test_tier_from_model_name() {
        assert_eq!(ComplexityTier::from_model_name("claude-haiku-4-5"), Some(ComplexityTier::Low));
        assert_eq!(ComplexityTier::from_model_name("claude-sonnet-4-6"), Some(ComplexityTier::Medium));
        assert_eq!(ComplexityTier::from_model_name("claude-opus-4-6"), Some(ComplexityTier::High));
        assert_eq!(ComplexityTier::from_model_name("gpt-4"), None);
    }
}
