//! 智能模型路由模块
//!
//! 根据请求复杂度自动选择合适的模型层级（Haiku/Sonnet/Opus）
//! 参考 oh-my-claudecode 的智能路由实现
//!
//! ## 核心特性
//! - 三层信号系统：词汇信号、结构信号、上下文信号
//! - 规则引擎：基于优先级的规则匹配
//! - 权重配置：可调优的权重系统

use serde::{Deserialize, Serialize};
use serde_json::Value;

// =============================================================================
// 类型定义
// =============================================================================

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

/// 问题深度类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuestionDepth {
    #[default]
    None,
    Where,
    What,
    How,
    Why,
}

/// 影响范围
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImpactScope {
    #[default]
    Local,
    Module,
    SystemWide,
}

/// 可逆性评估
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reversibility {
    #[default]
    Easy,
    Moderate,
    Difficult,
}

/// 领域特定性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DomainSpecificity {
    #[default]
    Generic,
    Frontend,
    Backend,
    Infrastructure,
    Security,
}

/// 词汇信号
#[derive(Debug, Default)]
pub struct LexicalSignals {
    /// 单词数量
    pub word_count: usize,
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
    /// 问题深度
    pub question_depth: QuestionDepth,
    /// 是否有隐式需求（模糊描述）
    pub has_implicit_requirements: bool,
}

/// 结构信号
#[derive(Debug, Default)]
pub struct StructuralSignals {
    /// 估计的子任务数量
    pub estimated_subtasks: usize,
    /// 是否有跨文件依赖
    pub cross_file_dependencies: bool,
    /// 是否需要测试
    pub has_test_requirements: bool,
    /// 领域特定性
    pub domain_specificity: DomainSpecificity,
    /// 是否需要外部知识
    pub requires_external_knowledge: bool,
    /// 可逆性评估
    pub reversibility: Reversibility,
    /// 影响范围
    pub impact_scope: ImpactScope,
}

/// 上下文信号
#[derive(Debug, Default)]
pub struct ContextSignals {
    /// 消息数量
    pub message_count: usize,
    /// 工具调用数量
    pub tool_count: usize,
    /// 是否启用 thinking
    pub has_thinking: bool,
    /// 系统提示长度
    pub system_prompt_chars: usize,
    /// 是否有长对话历史
    pub has_long_history: bool,
}

/// 复杂度信号（组合）
#[derive(Debug, Default)]
pub struct ComplexitySignals {
    pub lexical: LexicalSignals,
    pub structural: StructuralSignals,
    pub context: ContextSignals,
}

/// 路由决策结果
#[derive(Debug)]
#[allow(dead_code)]
pub struct RoutingDecision {
    /// 目标层级
    pub tier: ComplexityTier,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 决策原因
    pub reasons: Vec<String>,
    /// 匹配的规则名称
    pub matched_rule: Option<String>,
}

// =============================================================================
// 权重配置
// =============================================================================

/// 词汇信号权重
mod weights {
    pub mod lexical {
        pub const WORD_COUNT_HIGH: i32 = 2;
        pub const WORD_COUNT_VERY_HIGH: i32 = 1;
        pub const FILE_PATHS_MULTIPLE: i32 = 1;
        pub const CODE_BLOCKS_PRESENT: i32 = 1;
        pub const ARCHITECTURE_KEYWORDS: i32 = 3;
        pub const DEBUGGING_KEYWORDS: i32 = 2;
        pub const SIMPLE_KEYWORDS: i32 = -2;
        pub const RISK_KEYWORDS: i32 = 2;
        pub const QUESTION_WHY: i32 = 2;
        pub const QUESTION_HOW: i32 = 1;
        pub const IMPLICIT_REQUIREMENTS: i32 = 1;
    }

    pub mod structural {
        pub const SUBTASKS_MANY: i32 = 3;
        pub const SUBTASKS_SOME: i32 = 1;
        pub const CROSS_FILE: i32 = 2;
        pub const TEST_REQUIRED: i32 = 1;
        pub const SECURITY_DOMAIN: i32 = 2;
        pub const INFRASTRUCTURE_DOMAIN: i32 = 1;
        pub const EXTERNAL_KNOWLEDGE: i32 = 1;
        pub const REVERSIBILITY_DIFFICULT: i32 = 2;
        pub const REVERSIBILITY_MODERATE: i32 = 1;
        pub const IMPACT_SYSTEM_WIDE: i32 = 3;
        pub const IMPACT_MODULE: i32 = 1;
    }

    pub mod context {
        pub const MESSAGES_MANY: i32 = 2;
        pub const MESSAGES_SOME: i32 = 1;
        pub const TOOLS_MANY: i32 = 3;
        pub const TOOLS_SOME: i32 = 2;
        pub const TOOLS_FEW: i32 = 1;
        pub const THINKING: i32 = 2;
        pub const SYSTEM_PROMPT_LONG: i32 = 2;
        pub const SYSTEM_PROMPT_MEDIUM: i32 = 1;
        pub const LONG_HISTORY: i32 = 1;
    }
}

// =============================================================================
// 关键词定义
// =============================================================================

mod keywords {
    pub const ARCHITECTURE: &[&str] = &[
        "architecture", "refactor", "redesign", "restructure", "migrate",
        "design pattern", "system design", "decouple", "modularize", "abstract",
        "架构", "重构", "迁移", "设计模式", "解耦", "模块化",
    ];

    pub const DEBUGGING: &[&str] = &[
        "debug", "diagnose", "root cause", "investigate", "trace",
        "fix bug", "error", "exception", "stack trace", "why is",
        "figure out", "understand why", "not working",
        "调试", "诊断", "排查", "修复", "错误", "为什么",
    ];

    pub const SIMPLE: &[&str] = &[
        "find", "search", "locate", "list", "show", "where is",
        "what is", "get", "read", "fetch", "display", "print",
        "查找", "搜索", "列出", "显示", "是什么", "读取",
    ];

    pub const RISK: &[&str] = &[
        "critical", "production", "urgent", "security", "breaking",
        "dangerous", "irreversible", "data loss", "migration", "deploy",
        "核心", "重要", "紧急", "安全", "生产",
    ];

    pub const IMPLICIT: &[&str] = &[
        "make it better", "improve", "fix", "optimize", "clean up", "refactor",
    ];

    pub const SYSTEM_WIDE: &[&str] = &[
        "entire", "all files", "whole project", "whole system",
        "system-wide", "global", "everywhere", "throughout",
        "整个", "所有文件", "全部", "全局",
    ];

    pub const MODULE: &[&str] = &[
        "module", "package", "service", "feature", "component", "layer",
        "模块", "组件", "服务",
    ];

    pub const DIFFICULT_REVERSIBILITY: &[&str] = &[
        "migrate", "production", "data loss", "delete all", "drop table",
        "irreversible", "permanent",
        "迁移", "删除", "永久",
    ];

    pub const MODERATE_REVERSIBILITY: &[&str] = &[
        "refactor", "restructure", "rename across", "move files", "change schema",
        "重构", "重命名", "修改",
    ];
}

// =============================================================================
// 路由规则
// =============================================================================

/// 路由规则
#[derive(Clone)]
pub struct RoutingRule {
    /// 规则名称
    pub name: &'static str,
    /// 条件函数
    pub condition: fn(&ComplexitySignals) -> bool,
    /// 目标层级
    pub tier: ComplexityTier,
    /// 原因
    pub reason: &'static str,
    /// 优先级（越高越先匹配）
    pub priority: i32,
}

/// 默认路由规则（按优先级排序）
const DEFAULT_ROUTING_RULES: &[RoutingRule] = &[
    // ============ 高优先级规则 ============
    // 架构 + 系统级影响
    RoutingRule {
        name: "architecture-system-wide",
        condition: |s| s.lexical.has_architecture_keywords && s.structural.impact_scope == ImpactScope::SystemWide,
        tier: ComplexityTier::High,
        reason: "架构决策影响整个系统",
        priority: 70,
    },
    // 安全领域
    RoutingRule {
        name: "security-domain",
        condition: |s| s.structural.domain_specificity == DomainSpecificity::Security,
        tier: ComplexityTier::High,
        reason: "安全相关任务需要仔细推理",
        priority: 70,
    },
    // 高风险 + 难以逆转
    RoutingRule {
        name: "difficult-reversibility-risk",
        condition: |s| s.structural.reversibility == Reversibility::Difficult && s.lexical.has_risk_keywords,
        tier: ComplexityTier::High,
        reason: "高风险、难以逆转的变更",
        priority: 70,
    },
    // 深度调试
    RoutingRule {
        name: "deep-debugging",
        condition: |s| s.lexical.has_debugging_keywords && s.lexical.question_depth == QuestionDepth::Why,
        tier: ComplexityTier::High,
        reason: "根因分析需要深度推理",
        priority: 65,
    },
    // 复杂多步骤
    RoutingRule {
        name: "complex-multi-step",
        condition: |s| s.structural.estimated_subtasks > 5 && s.structural.cross_file_dependencies,
        tier: ComplexityTier::High,
        reason: "复杂多步骤任务涉及跨文件变更",
        priority: 60,
    },
    // 简单查询
    RoutingRule {
        name: "simple-search-query",
        condition: |s| {
            s.lexical.has_simple_keywords &&
            s.structural.estimated_subtasks <= 1 &&
            s.structural.impact_scope == ImpactScope::Local &&
            !s.lexical.has_architecture_keywords &&
            !s.lexical.has_debugging_keywords
        },
        tier: ComplexityTier::Low,
        reason: "简单搜索或查找任务",
        priority: 60,
    },
    // 短小的本地变更
    RoutingRule {
        name: "short-local-change",
        condition: |s| {
            s.lexical.word_count < 50 &&
            s.structural.impact_scope == ImpactScope::Local &&
            s.structural.reversibility == Reversibility::Easy &&
            !s.lexical.has_risk_keywords
        },
        tier: ComplexityTier::Low,
        reason: "简短、本地、易于逆转的变更",
        priority: 55,
    },
    // 中等复杂度
    RoutingRule {
        name: "moderate-complexity",
        condition: |s| s.structural.estimated_subtasks > 1 && s.structural.estimated_subtasks <= 5,
        tier: ComplexityTier::Medium,
        reason: "中等复杂度，多个子任务",
        priority: 50,
    },
    // 模块级工作
    RoutingRule {
        name: "module-level-work",
        condition: |s| s.structural.impact_scope == ImpactScope::Module,
        tier: ComplexityTier::Medium,
        reason: "模块级别的变更",
        priority: 45,
    },
    // 默认规则
    RoutingRule {
        name: "default-medium",
        condition: |_| true,
        tier: ComplexityTier::Medium,
        reason: "默认中等层级",
        priority: 0,
    },
];

// =============================================================================
// 配置
// =============================================================================

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
    /// 是否启用混合策略（尊重原始模型层级）
    #[serde(default = "default_true")]
    pub hybrid_mode: bool,
    /// 最多升级几级（0 = 禁止升级，1 = 最多升一级，2 = 允许跨两级）
    #[serde(default = "default_max_upgrade_steps")]
    pub max_upgrade_steps: u8,
    /// 最多降级几级（0 = 禁止降级，1 = 最多降一级，2 = 允许跨两级）
    #[serde(default = "default_max_downgrade_steps")]
    pub max_downgrade_steps: u8,
    /// opus 请求是否永不降级（安全考虑）
    #[serde(default)]
    pub never_downgrade_opus: bool,
}

fn default_true() -> bool {
    true
}

fn default_max_upgrade_steps() -> u8 {
    1
}

fn default_max_downgrade_steps() -> u8 {
    1
}

impl Default for SmartRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_tier: None,
            max_tier: None,
            low_threshold: 4,
            high_threshold: 8,
            hybrid_mode: true,
            max_upgrade_steps: 1,
            max_downgrade_steps: 1,
            never_downgrade_opus: false,
        }
    }
}

// =============================================================================
// 信号提取
// =============================================================================

/// 从请求体提取复杂度信号
///
/// 只分析最近的用户消息，避免历史消息干扰复杂度判断
pub fn extract_signals(body: &Value) -> ComplexitySignals {
    let mut signals = ComplexitySignals::default();

    // 提取消息
    let messages = body.get("messages").and_then(|m| m.as_array());

    if let Some(messages) = messages {
        signals.context.message_count = messages.len();
        signals.context.has_long_history = messages.len() > 10;

        // 只分析最近的用户消息（通常是最后一条或倒数第二条）
        // 这样可以避免历史消息中的工具调用影响复杂度判断
        let recent_user_message = messages
            .iter()
            .rev()
            .find(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("user"));

        let mut user_text = String::new();
        if let Some(msg) = recent_user_message {
            if let Some(content) = msg.get("content") {
                user_text = extract_text_from_content(content);

                // 只统计最近用户消息中的工具调用
                if let Some(content_arr) = content.as_array() {
                    for block in content_arr {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            signals.context.tool_count += 1;
                        }
                    }
                }
            }
        }

        // 提取词汇信号（只基于最近用户消息）
        signals.lexical = extract_lexical_signals(&user_text);

        // 提取结构信号（只基于最近用户消息）
        signals.structural = extract_structural_signals(&user_text);
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
        signals.context.system_prompt_chars = system_text.chars().count();
    }

    // thinking 模式
    signals.context.has_thinking = super::model_mapper::has_thinking_enabled(body);

    signals
}

/// 提取词汇信号
fn extract_lexical_signals(text: &str) -> LexicalSignals {
    let mut signals = LexicalSignals::default();

    // 统计单词数量
    signals.word_count = text.split_whitespace().count();

    // 统计文件路径
    signals.file_path_count = count_file_paths(text);

    // 统计代码块
    signals.code_block_count = text.matches("```").count() / 2;

    // 检测关键词
    let text_lower = text.to_lowercase();
    signals.has_architecture_keywords =
        keywords::ARCHITECTURE.iter().any(|k| text_lower.contains(k));
    signals.has_debugging_keywords =
        keywords::DEBUGGING.iter().any(|k| text_lower.contains(k));
    signals.has_simple_keywords =
        keywords::SIMPLE.iter().any(|k| text_lower.contains(k));
    signals.has_risk_keywords =
        keywords::RISK.iter().any(|k| text_lower.contains(k));

    // 问题深度
    signals.question_depth = detect_question_depth(&text_lower);

    // 隐式需求
    signals.has_implicit_requirements = detect_implicit_requirements(&text_lower);

    signals
}

/// 提取结构信号
fn extract_structural_signals(text: &str) -> StructuralSignals {
    let mut signals = StructuralSignals::default();

    // 估计子任务数量
    signals.estimated_subtasks = estimate_subtasks(text);

    // 跨文件依赖
    signals.cross_file_dependencies = detect_cross_file_dependencies(text);

    // 测试需求
    signals.has_test_requirements = detect_test_requirements(text);

    // 领域特定性
    signals.domain_specificity = detect_domain(text);

    // 外部知识需求
    signals.requires_external_knowledge = detect_external_knowledge(text);

    // 可逆性
    signals.reversibility = assess_reversibility(text);

    // 影响范围
    signals.impact_scope = assess_impact_scope(text);

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
    let patterns = [
        r"[A-Za-z]:[/\\][\w\-./\\]+\.\w{1,10}",
        r"/[\w\-./]+\.\w{1,10}",
        r"\.{1,2}/[\w\-./]+\.\w{1,10}",
        r"\b[\w\-]+\.\w{1,10}\b",
    ];

    let mut count = 0;
    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            count += re.find_iter(text).count();
        }
    }

    count.min(text.len() / 5)
}

/// 检测问题深度
fn detect_question_depth(text_lower: &str) -> QuestionDepth {
    if text_lower.contains("why") || text_lower.contains("为什么") {
        return QuestionDepth::Why;
    }
    if text_lower.contains("how") || text_lower.contains("如何") || text_lower.contains("怎么") {
        return QuestionDepth::How;
    }
    if text_lower.contains("what") || text_lower.contains("是什么") {
        return QuestionDepth::What;
    }
    if text_lower.contains("where") || text_lower.contains("在哪里") {
        return QuestionDepth::Where;
    }
    QuestionDepth::None
}

/// 检测隐式需求
fn detect_implicit_requirements(text_lower: &str) -> bool {
    keywords::IMPLICIT.iter().any(|k| text_lower.contains(k))
}

/// 估计子任务数量
fn estimate_subtasks(text: &str) -> usize {
    let mut count = 1;

    // 列表项
    let bullet_points = (text.matches("- ").count() + text.matches("* ").count() + text.matches("• ").count()).min(5);
    count += bullet_points;

    // 编号项
    let numbered = regex::Regex::new(r"\d+[.)]\s")
        .map(|re| re.find_iter(text).count())
        .unwrap_or(0)
        .min(5);
    count += numbered;

    // "and" 连接词
    let and_count = text.matches(" and ").count() + text.matches("和").count();
    count += and_count / 2;

    // "then" 顺序词
    let then_count = text.matches(" then ").count() + text.matches("然后").count();
    count += then_count;

    count.min(10)
}

/// 检测跨文件依赖
fn detect_cross_file_dependencies(text: &str) -> bool {
    if count_file_paths(text) >= 2 {
        return true;
    }

    let indicators = [
        "multiple files", "across files", "several files", "all files",
        "entire project", "whole system", "多个文件", "跨文件",
    ];

    let text_lower = text.to_lowercase();
    indicators.iter().any(|i| text_lower.contains(i))
}

/// 检测测试需求
fn detect_test_requirements(text: &str) -> bool {
    let indicators = [
        "test", "spec", "make sure", "verify", "ensure", "tdd",
        "unit test", "integration test",
        "测试", "验证", "确保",
    ];

    let text_lower = text.to_lowercase();
    indicators.iter().any(|i| text_lower.contains(i))
}

/// 检测领域
fn detect_domain(text: &str) -> DomainSpecificity {
    let text_lower = text.to_lowercase();

    let security_patterns = [
        "security", "auth", "oauth", "jwt", "encryption", "vulnerability",
        "xss", "csrf", "injection", "password", "credential", "secret", "token",
        "安全", "认证", "加密", "密码",
    ];
    if security_patterns.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Security;
    }

    let infra_patterns = [
        "docker", "kubernetes", "k8s", "terraform", "aws", "gcp", "azure",
        "ci", "cd", "deploy", "container", "nginx", "load balancer",
        "容器", "部署", "集群",
    ];
    if infra_patterns.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Infrastructure;
    }

    let frontend_patterns = [
        "react", "vue", "angular", "svelte", "css", "html", "jsx", "tsx",
        "component", "ui", "ux", "styling", "tailwind", "button", "modal", "form",
        "组件", "界面", "样式",
    ];
    if frontend_patterns.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Frontend;
    }

    let backend_patterns = [
        "api", "endpoint", "database", "query", "sql", "graphql", "rest",
        "server", "middleware", "node", "express", "django", "flask",
        "接口", "数据库", "服务端",
    ];
    if backend_patterns.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Backend;
    }

    DomainSpecificity::Generic
}

/// 检测外部知识需求
fn detect_external_knowledge(text: &str) -> bool {
    let indicators = [
        "docs", "documentation", "official", "library", "package", "framework",
        "how does", "best practice",
        "文档", "官方", "库", "框架", "最佳实践",
    ];

    let text_lower = text.to_lowercase();
    indicators.iter().any(|i| text_lower.contains(i))
}

/// 评估可逆性
fn assess_reversibility(text: &str) -> Reversibility {
    let text_lower = text.to_lowercase();

    if keywords::DIFFICULT_REVERSIBILITY.iter().any(|k| text_lower.contains(k)) {
        return Reversibility::Difficult;
    }

    if keywords::MODERATE_REVERSIBILITY.iter().any(|k| text_lower.contains(k)) {
        return Reversibility::Moderate;
    }

    Reversibility::Easy
}

/// 评估影响范围
fn assess_impact_scope(text: &str) -> ImpactScope {
    let text_lower = text.to_lowercase();

    if keywords::SYSTEM_WIDE.iter().any(|k| text_lower.contains(k)) {
        return ImpactScope::SystemWide;
    }

    if count_file_paths(text) >= 3 {
        return ImpactScope::Module;
    }

    if keywords::MODULE.iter().any(|k| text_lower.contains(k)) {
        return ImpactScope::Module;
    }

    ImpactScope::Local
}

// =============================================================================
// 评分计算
// =============================================================================

/// 计算词汇信号分数
fn score_lexical_signals(signals: &LexicalSignals) -> i32 {
    let mut score = 0;

    use weights::lexical::*;

    if signals.word_count > 200 {
        score += WORD_COUNT_HIGH;
        if signals.word_count > 500 {
            score += WORD_COUNT_VERY_HIGH;
        }
    }

    if signals.file_path_count >= 2 {
        score += FILE_PATHS_MULTIPLE;
    }

    if signals.code_block_count > 0 {
        score += CODE_BLOCKS_PRESENT;
    }

    if signals.has_architecture_keywords {
        score += ARCHITECTURE_KEYWORDS;
    }
    if signals.has_debugging_keywords {
        score += DEBUGGING_KEYWORDS;
    }
    if signals.has_simple_keywords {
        score += SIMPLE_KEYWORDS; // 负数
    }
    if signals.has_risk_keywords {
        score += RISK_KEYWORDS;
    }

    match signals.question_depth {
        QuestionDepth::Why => score += QUESTION_WHY,
        QuestionDepth::How => score += QUESTION_HOW,
        _ => {}
    }

    if signals.has_implicit_requirements {
        score += IMPLICIT_REQUIREMENTS;
    }

    score
}

/// 计算结构信号分数
fn score_structural_signals(signals: &StructuralSignals) -> i32 {
    let mut score = 0;

    use weights::structural::*;

    if signals.estimated_subtasks > 3 {
        score += SUBTASKS_MANY;
    } else if signals.estimated_subtasks > 1 {
        score += SUBTASKS_SOME;
    }

    if signals.cross_file_dependencies {
        score += CROSS_FILE;
    }

    if signals.has_test_requirements {
        score += TEST_REQUIRED;
    }

    match signals.domain_specificity {
        DomainSpecificity::Security => score += SECURITY_DOMAIN,
        DomainSpecificity::Infrastructure => score += INFRASTRUCTURE_DOMAIN,
        _ => {}
    }

    if signals.requires_external_knowledge {
        score += EXTERNAL_KNOWLEDGE;
    }

    match signals.reversibility {
        Reversibility::Difficult => score += REVERSIBILITY_DIFFICULT,
        Reversibility::Moderate => score += REVERSIBILITY_MODERATE,
        _ => {}
    }

    match signals.impact_scope {
        ImpactScope::SystemWide => score += IMPACT_SYSTEM_WIDE,
        ImpactScope::Module => score += IMPACT_MODULE,
        _ => {}
    }

    score
}

/// 计算上下文信号分数
fn score_context_signals(signals: &ContextSignals) -> i32 {
    let mut score = 0;

    use weights::context::*;

    if signals.message_count > 20 {
        score += MESSAGES_MANY;
    } else if signals.message_count > 10 {
        score += MESSAGES_SOME;
    }

    if signals.tool_count > 10 {
        score += TOOLS_MANY;
    } else if signals.tool_count > 5 {
        score += TOOLS_SOME;
    } else if signals.tool_count > 2 {
        score += TOOLS_FEW;
    }

    if signals.has_thinking {
        score += THINKING;
    }

    if signals.system_prompt_chars > 5000 {
        score += SYSTEM_PROMPT_LONG;
    } else if signals.system_prompt_chars > 2000 {
        score += SYSTEM_PROMPT_MEDIUM;
    }

    if signals.has_long_history {
        score += LONG_HISTORY;
    }

    score
}

/// 计算总复杂度分数
pub fn calculate_score(signals: &ComplexitySignals) -> i32 {
    let lexical = score_lexical_signals(&signals.lexical);
    let structural = score_structural_signals(&signals.structural);
    let context = score_context_signals(&signals.context);

    lexical + structural + context
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

/// 计算置信度
fn calculate_confidence(score: i32, tier: ComplexityTier, config: &SmartRouterConfig) -> f32 {
    let distance_from_low = (score - config.low_threshold).abs();
    let distance_from_high = (score - config.high_threshold).abs();

    let min_distance = match tier {
        ComplexityTier::Low => config.low_threshold - score,
        ComplexityTier::Medium => distance_from_low.min(distance_from_high),
        ComplexityTier::High => score - config.high_threshold,
    };

    // 距离 0 = 0.5 置信度，距离 4+ = 0.9+ 置信度
    let confidence = 0.5 + (min_distance.min(4) as f32 / 4.0) * 0.4;
    (confidence * 100.0).round() / 100.0
}

// =============================================================================
// 路由决策
// =============================================================================

/// 评估规则
fn evaluate_rules(signals: &ComplexitySignals) -> Option<(&'static str, ComplexityTier, &'static str)> {
    // 按优先级排序评估规则
    let mut rules: Vec<_> = DEFAULT_ROUTING_RULES.to_vec();
    rules.sort_by(|a, b| b.priority.cmp(&a.priority));

    for rule in rules {
        if (rule.condition)(signals) {
            return Some((rule.name, rule.tier, rule.reason));
        }
    }

    None
}

/// 智能路由决策
pub fn route(body: &Value, config: &SmartRouterConfig) -> RoutingDecision {
    if !config.enabled {
        return RoutingDecision {
            tier: ComplexityTier::Medium,
            confidence: 1.0,
            reasons: vec!["智能路由已禁用".to_string()],
            matched_rule: None,
        };
    }

    // 提取信号
    let signals = extract_signals(body);

    // 计算分数
    let score = calculate_score(&signals);
    let score_tier = score_to_tier(score, config);

    // 评估规则
    let rule_result = evaluate_rules(&signals);

    let (final_tier, matched_rule, mut reasons) = if let Some((name, tier, reason)) = rule_result {
        let mut reasons = vec![
            format!("复杂度分数: {}", score),
            format!("规则: {} - {}", name, reason),
        ];

        // 如果规则和评分差异大，选择更高的层级
        let tier_order = [ComplexityTier::Low, ComplexityTier::Medium, ComplexityTier::High];
        let rule_idx = tier_order.iter().position(|&t| t == tier).unwrap_or(1);
        let score_idx = tier_order.iter().position(|&t| t == score_tier).unwrap_or(1);

        let final_tier = if (rule_idx as i32 - score_idx as i32).abs() > 1 {
            // 差异大时，选择更高的层级以避免能力不足
            reasons.push("规则与评分差异较大，选择更高层级".to_string());
            tier_order[rule_idx.max(score_idx)]
        } else {
            tier
        };

        (final_tier, Some(name), reasons)
    } else {
        (
            score_tier,
            None,
            vec![format!("复杂度分数: {}", score)],
        )
    };

    // 收集详细原因
    if signals.lexical.has_architecture_keywords {
        reasons.push("包含架构关键词".to_string());
    }
    if signals.lexical.has_debugging_keywords {
        reasons.push("包含调试关键词".to_string());
    }
    if signals.lexical.has_risk_keywords {
        reasons.push("包含风险关键词".to_string());
    }
    if signals.lexical.has_simple_keywords {
        reasons.push("包含简单关键词".to_string());
    }
    if signals.structural.impact_scope == ImpactScope::SystemWide {
        reasons.push("系统级影响范围".to_string());
    }
    if signals.structural.reversibility == Reversibility::Difficult {
        reasons.push("难以逆转的变更".to_string());
    }
    if signals.context.tool_count > 5 {
        reasons.push(format!("工具调用较多 ({})", signals.context.tool_count));
    }
    if signals.context.has_thinking {
        reasons.push("启用 thinking 模式".to_string());
    }

    // 计算置信度
    let confidence = calculate_confidence(score, final_tier, config);

    RoutingDecision {
        tier: final_tier,
        confidence,
        reasons,
        matched_rule: matched_rule.map(|s| s.to_string()),
    }
}

// =============================================================================
// 测试
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // ComplexityTier 序列化/反序列化
    // =========================================================================

    #[test]
    fn test_complexity_tier_serialize() {
        assert_eq!(serde_json::to_string(&ComplexityTier::Low).unwrap(), r#""low""#);
        assert_eq!(serde_json::to_string(&ComplexityTier::Medium).unwrap(), r#""medium""#);
        assert_eq!(serde_json::to_string(&ComplexityTier::High).unwrap(), r#""high""#);
    }

    #[test]
    fn test_complexity_tier_deserialize() {
        assert_eq!(
            serde_json::from_str::<ComplexityTier>(r#""low""#).unwrap(),
            ComplexityTier::Low
        );
        assert_eq!(
            serde_json::from_str::<ComplexityTier>(r#""medium""#).unwrap(),
            ComplexityTier::Medium
        );
        assert_eq!(
            serde_json::from_str::<ComplexityTier>(r#""high""#).unwrap(),
            ComplexityTier::High
        );
    }

    #[test]
    fn test_complexity_tier_ordering() {
        assert!(ComplexityTier::High > ComplexityTier::Medium);
        assert!(ComplexityTier::Medium > ComplexityTier::Low);
        assert!(ComplexityTier::High > ComplexityTier::Low);
    }

    #[test]
    fn test_complexity_tier_default() {
        assert_eq!(ComplexityTier::default(), ComplexityTier::Medium);
    }

    // =========================================================================
    // SmartRouterConfig 默认值
    // =========================================================================

    #[test]
    fn test_config_default_values() {
        let config = SmartRouterConfig::default();
        assert!(config.enabled);
        assert!(config.min_tier.is_none());
        assert!(config.max_tier.is_none());
        assert_eq!(config.low_threshold, 4);
        assert_eq!(config.high_threshold, 8);
        assert!(config.hybrid_mode);
        assert_eq!(config.max_upgrade_steps, 1);
        assert_eq!(config.max_downgrade_steps, 1);
        assert!(!config.never_downgrade_opus);
    }

    #[test]
    fn test_config_serialize_roundtrip() {
        let config = SmartRouterConfig {
            enabled: false,
            min_tier: Some(ComplexityTier::Medium),
            max_tier: Some(ComplexityTier::High),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: SmartRouterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.enabled, config.enabled);
        assert_eq!(restored.min_tier, config.min_tier);
        assert_eq!(restored.max_tier, config.max_tier);
    }

    // =========================================================================
    // extract_signals 词汇信号
    // =========================================================================

    #[test]
    fn test_extract_signals_simple_query() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "查找文件 test.rs"}
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.lexical.has_simple_keywords);
        assert!(!signals.lexical.has_architecture_keywords);
        assert_eq!(signals.structural.impact_scope, ImpactScope::Local);
    }

    #[test]
    fn test_extract_signals_architecture() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "请重构这个架构设计模式"}
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.lexical.has_architecture_keywords);
    }

    #[test]
    fn test_extract_signals_system_wide() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "重构整个项目的架构"}
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.lexical.has_architecture_keywords);
        assert_eq!(signals.structural.impact_scope, ImpactScope::SystemWide);
    }

    #[test]
    fn test_extract_signals_with_code_blocks() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "```\ncode here\n```\nand ```\nmore code\n```"}
            ]
        });

        let signals = extract_signals(&body);
        assert_eq!(signals.lexical.code_block_count, 2);
    }

    #[test]
    fn test_extract_signals_word_count() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "one two three four five"}
            ]
        });

        let signals = extract_signals(&body);
        assert_eq!(signals.lexical.word_count, 5);
    }

    #[test]
    fn test_extract_signals_empty_input() {
        let body = json!({
            "messages": []
        });

        let signals = extract_signals(&body);
        assert_eq!(signals.lexical.word_count, 0);
        assert_eq!(signals.context.message_count, 0);
    }

    #[test]
    fn test_extract_signals_array_content() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "查找文件并重构架构"}
                    ]
                }
            ]
        });

        let signals = extract_signals(&body);
        assert!(signals.lexical.has_simple_keywords);
        assert!(signals.lexical.has_architecture_keywords);
    }

    #[test]
    fn test_extract_signals_tool_count() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_use", "id": "tool1"},
                        {"type": "tool_use", "id": "tool2"},
                        {"type": "text", "text": "help"}
                    ]
                }
            ]
        });

        let signals = extract_signals(&body);
        assert_eq!(signals.context.tool_count, 2);
    }

    // =========================================================================
    // 问题深度检测
    // =========================================================================

    #[test]
    fn test_question_depth_english() {
        assert_eq!(detect_question_depth("why is this happening?"), QuestionDepth::Why);
        assert_eq!(detect_question_depth("how does this work?"), QuestionDepth::How);
        assert_eq!(detect_question_depth("what is this?"), QuestionDepth::What);
        assert_eq!(detect_question_depth("where is the file?"), QuestionDepth::Where);
        assert_eq!(detect_question_depth("show me the code"), QuestionDepth::None);
    }

    #[test]
    fn test_question_depth_chinese() {
        assert_eq!(detect_question_depth("为什么会这样？"), QuestionDepth::Why);
        assert_eq!(detect_question_depth("如何实现？"), QuestionDepth::How);
        assert_eq!(detect_question_depth("是什么？"), QuestionDepth::What);
        assert_eq!(detect_question_depth("在哪里？"), QuestionDepth::Where);
    }

    #[test]
    fn test_question_depth_case_insensitive() {
        assert_eq!(detect_question_depth("WHY"), QuestionDepth::Why);
        assert_eq!(detect_question_depth("Why"), QuestionDepth::Why);
        assert_eq!(detect_question_depth("为什么"), QuestionDepth::Why);
    }

    // =========================================================================
    // 领域检测
    // =========================================================================

    #[test]
    fn test_domain_detection_security() {
        assert_eq!(detect_domain("fix the security vulnerability"), DomainSpecificity::Security);
        assert_eq!(detect_domain("add oauth authentication"), DomainSpecificity::Security);
        assert_eq!(detect_domain("encrypt the password"), DomainSpecificity::Security);
        assert_eq!(detect_domain("check for XSS and CSRF"), DomainSpecificity::Security);
    }

    #[test]
    fn test_domain_detection_infrastructure() {
        assert_eq!(detect_domain("deploy to kubernetes"), DomainSpecificity::Infrastructure);
        assert_eq!(detect_domain("build docker image"), DomainSpecificity::Infrastructure);
        assert_eq!(detect_domain("setup nginx load balancer"), DomainSpecificity::Infrastructure);
    }

    #[test]
    fn test_domain_detection_frontend() {
        assert_eq!(detect_domain("update the react component"), DomainSpecificity::Frontend);
        assert_eq!(detect_domain("fix css styling"), DomainSpecificity::Frontend);
        assert_eq!(detect_domain("create vue component"), DomainSpecificity::Frontend);
    }

    #[test]
    fn test_domain_detection_backend() {
        assert_eq!(detect_domain("fix the API endpoint"), DomainSpecificity::Backend);
        assert_eq!(detect_domain("write SQL query"), DomainSpecificity::Backend);
        assert_eq!(detect_domain("setup REST API"), DomainSpecificity::Backend);
    }

    #[test]
    fn test_domain_detection_generic() {
        assert_eq!(detect_domain("update the readme"), DomainSpecificity::Generic);
        assert_eq!(detect_domain("write documentation"), DomainSpecificity::Generic);
    }

    // =========================================================================
    // 可逆性评估
    // =========================================================================

    #[test]
    fn test_reversibility_difficult() {
        assert_eq!(assess_reversibility("migrate database"), Reversibility::Difficult);
        assert_eq!(assess_reversibility("deploy to production"), Reversibility::Difficult);
        assert_eq!(assess_reversibility("delete all files"), Reversibility::Difficult);
    }

    #[test]
    fn test_reversibility_moderate() {
        assert_eq!(assess_reversibility("refactor code"), Reversibility::Moderate);
        assert_eq!(assess_reversibility("rename across files"), Reversibility::Moderate);
    }

    #[test]
    fn test_reversibility_easy() {
        assert_eq!(assess_reversibility("add a function"), Reversibility::Easy);
        assert_eq!(assess_reversibility("update comment"), Reversibility::Easy);
    }

    // =========================================================================
    // 影响范围评估
    // =========================================================================

    #[test]
    fn test_impact_scope_system_wide() {
        assert_eq!(assess_impact_scope("refactor entire system"), ImpactScope::SystemWide);
        assert_eq!(assess_impact_scope("change across all files"), ImpactScope::SystemWide);
        assert_eq!(assess_impact_scope("全局修改"), ImpactScope::SystemWide);
    }

    #[test]
    fn test_impact_scope_module() {
        assert_eq!(assess_impact_scope("update the auth module"), ImpactScope::Module);
        assert_eq!(assess_impact_scope("修改组件"), ImpactScope::Module);
        // 多个文件路径也认为是模块级
        let text = "modify src/main.rs and src/lib.rs and src/util.rs";
        assert_eq!(assess_impact_scope(text), ImpactScope::Module);
    }

    #[test]
    fn test_impact_scope_local() {
        assert_eq!(assess_impact_scope("fix this function"), ImpactScope::Local);
        assert_eq!(assess_impact_scope("update variable name"), ImpactScope::Local);
    }

    // =========================================================================
    // 估计子任务数量
    // =========================================================================

    #[test]
    fn test_estimate_subtasks_bullets() {
        let text = "- task one\n- task two\n- task three";
        assert_eq!(estimate_subtasks(text), 4); // 1 + 3 bullets
    }

    #[test]
    fn test_estimate_subtasks_numbered() {
        let text = "1. first\n2. second\n3. third";
        assert!(estimate_subtasks(text) >= 4);
    }

    #[test]
    fn test_estimate_subtasks_and_connector() {
        let text = "do this and that and another";
        assert!(estimate_subtasks(text) > 1);
    }

    #[test]
    fn test_estimate_subtasks_capped() {
        let text = "- 1\n- 2\n- 3\n- 4\n- 5\n- 6\n- 7\n- 8\n- 9\n- 10\n- 11";
        assert_eq!(estimate_subtasks(text), 10); // capped at 10
    }

    // =========================================================================
    // 跨文件依赖检测
    // =========================================================================

    #[test]
    fn test_cross_file_by_path_count() {
        assert!(detect_cross_file_dependencies("src/main.rs src/lib.rs"));
    }

    #[test]
    fn test_cross_file_by_keyword() {
        assert!(detect_cross_file_dependencies("update across multiple files"));
    }

    #[test]
    fn test_cross_file_negative() {
        assert!(!detect_cross_file_dependencies("update single file"));
    }

    // =========================================================================
    // 测试需求检测
    // =========================================================================

    #[test]
    fn test_test_requirements() {
        assert!(detect_test_requirements("write unit tests"));
        assert!(detect_test_requirements("验证功能"));
        assert!(detect_test_requirements("ensure it works"));
        assert!(!detect_test_requirements("just implement"));
    }

    // =========================================================================
    // 外部知识需求检测
    // =========================================================================

    #[test]
    fn test_external_knowledge() {
        assert!(detect_external_knowledge("check the documentation"));
        assert!(detect_external_knowledge("参考官方文档"));
        assert!(detect_external_knowledge("what's the best practice"));
        assert!(!detect_external_knowledge("use local variable"));
    }

    // =========================================================================
    // 隐式需求检测
    // =========================================================================

    #[test]
    fn test_implicit_requirements() {
        assert!(detect_implicit_requirements("make it better"));
        assert!(detect_implicit_requirements("improve performance"));
        assert!(detect_implicit_requirements("fix the bug"));
        assert!(detect_implicit_requirements("优化代码"));
        assert!(!detect_implicit_requirements("add specific feature X"));
    }

    // =========================================================================
    // 评分计算
    // =========================================================================

    #[test]
    fn test_calculate_score_simple() {
        let signals = ComplexitySignals {
            lexical: LexicalSignals {
                has_simple_keywords: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let score = calculate_score(&signals);
        assert!(score < 0); // simple keywords subtract
    }

    #[test]
    fn test_calculate_score_complex() {
        let signals = ComplexitySignals {
            lexical: LexicalSignals {
                has_architecture_keywords: true,
                has_debugging_keywords: true,
                word_count: 250,
                file_path_count: 3,
                ..Default::default()
            },
            structural: StructuralSignals {
                impact_scope: ImpactScope::SystemWide,
                estimated_subtasks: 6,
                cross_file_dependencies: true,
                reversibility: Reversibility::Difficult,
                ..Default::default()
            },
            context: ContextSignals {
                tool_count: 8,
                has_thinking: true,
                ..Default::default()
            },
        };

        let score = calculate_score(&signals);
        assert!(score >= 8);
    }

    #[test]
    fn test_calculate_score_zero() {
        let signals = ComplexitySignals::default();
        let score = calculate_score(&signals);
        assert_eq!(score, 0);
    }

    // =========================================================================
    // score_to_tier 转换
    // =========================================================================

    #[test]
    fn test_score_to_tier_below_low_threshold() {
        let config = SmartRouterConfig {
            low_threshold: 5,
            high_threshold: 10,
            ..Default::default()
        };
        assert_eq!(score_to_tier(3, &config), ComplexityTier::Low);
    }

    #[test]
    fn test_score_to_tier_medium_range() {
        let config = SmartRouterConfig {
            low_threshold: 5,
            high_threshold: 10,
            ..Default::default()
        };
        assert_eq!(score_to_tier(7, &config), ComplexityTier::Medium);
    }

    #[test]
    fn test_score_to_tier_above_high_threshold() {
        let config = SmartRouterConfig {
            low_threshold: 5,
            high_threshold: 10,
            ..Default::default()
        };
        assert_eq!(score_to_tier(12, &config), ComplexityTier::High);
    }

    #[test]
    fn test_score_to_tier_with_min_constraint() {
        let config = SmartRouterConfig {
            low_threshold: 5,
            high_threshold: 10,
            min_tier: Some(ComplexityTier::Medium),
            ..Default::default()
        };
        // 即使分数很低，也应该返回 Medium
        assert_eq!(score_to_tier(0, &config), ComplexityTier::Medium);
    }

    #[test]
    fn test_score_to_tier_with_max_constraint() {
        let config = SmartRouterConfig {
            low_threshold: 5,
            high_threshold: 10,
            max_tier: Some(ComplexityTier::Medium),
            ..Default::default()
        };
        // 即使分数很高，也应该返回 Medium
        assert_eq!(score_to_tier(20, &config), ComplexityTier::Medium);
    }

    // =========================================================================
    // 置信度计算
    // =========================================================================

    #[test]
    fn test_confidence_calculation() {
        let config = SmartRouterConfig::default();

        // 边界情况
        let confidence_low = calculate_confidence(0, ComplexityTier::Low, &config);
        assert!(confidence_low >= 0.5 && confidence_low <= 1.0);

        let confidence_high = calculate_confidence(20, ComplexityTier::High, &config);
        assert!(confidence_high >= 0.5 && confidence_high <= 1.0);
    }

    // =========================================================================
    // 路由决策
    // =========================================================================

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
        assert!(decision.reasons.iter().any(|r| r.contains("简单")));
    }

    #[test]
    fn test_route_complex() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "请重构整个项目的架构，修复安全漏洞"},
                {"role": "assistant", "content": "好的，我来分析..."},
                {"role": "user", "content": "继续调试这个错误"},
            ],
            "thinking": {"type": "enabled"}
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::High);
    }

    #[test]
    fn test_route_disabled() {
        let config = SmartRouterConfig {
            enabled: false,
            ..Default::default()
        };
        let body = json!({
            "messages": [
                {"role": "user", "content": "复杂架构重构"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::Medium);
        assert_eq!(decision.confidence, 1.0);
        assert!(decision.reasons.iter().any(|r| r.contains("禁用")));
    }

    #[test]
    fn test_rule_matching() {
        let config = SmartRouterConfig::default();

        // 安全领域应该匹配 HIGH 规则
        let body = json!({
            "messages": [
                {"role": "user", "content": "修复安全漏洞"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::High);
        assert!(decision.matched_rule.is_some());
        assert!(decision.matched_rule.unwrap().contains("security"));
    }

    #[test]
    fn test_rule_deep_debugging() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "为什么这个代码会崩溃？我需要诊断根因"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::High);
    }

    #[test]
    fn test_rule_short_local_change() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "rename this variable"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::Low);
    }

    #[test]
    fn test_rule_moderate_complexity() {
        let config = SmartRouterConfig::default();
        let body = json!({
            "messages": [
                {"role": "user", "content": "- first task\n- second task\n- third task"}
            ]
        });

        let decision = route(&body, &config);
        assert_eq!(decision.tier, ComplexityTier::Medium);
    }

    #[test]
    fn test_route_with_long_history() {
        let _config = SmartRouterConfig::default();
        let mut messages = vec![];
        for i in 0..15 {
            messages.push(json!({"role": "user", "content": format!("message {}", i)}));
            messages.push(json!({"role": "assistant", "content": "ok"}));
        }
        messages.push(json!({"role": "user", "content": "continue"}));

        let body = json!({ "messages": messages });
        let signals = extract_signals(&body);
        assert!(signals.context.has_long_history);
    }

    // =========================================================================
    // 文件路径计数
    // =========================================================================

    #[test]
    fn test_count_file_paths_unix() {
        let count = count_file_paths("src/main.rs and src/lib.rs");
        assert!(count >= 2);
    }

    #[test]
    fn test_count_file_paths_windows() {
        let count = count_file_paths("C:\\Users\\dev\\project\\main.rs");
        assert!(count >= 1);
    }

    #[test]
    fn test_count_file_paths_relative() {
        let count = count_file_paths("./lib.rs and ../module.rs");
        assert!(count >= 2);
    }

    #[test]
    fn test_count_file_paths_capped() {
        // 应该有上限，防止过度计数
        let text = "a.rs b.rs c.rs d.rs e.rs f.rs g.rs h.rs i.rs j.rs k.rs l.rs m.rs n.rs o.rs p.rs";
        let count = count_file_paths(text);
        // cap 应该是 text.len() / 5 左右
        assert!(count < text.len());
    }
}
