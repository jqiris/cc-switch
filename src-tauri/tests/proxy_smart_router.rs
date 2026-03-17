/// 智能路由模块集成测试
///
/// 测试智能路由的核心功能，包括：
/// - 复杂度信号提取
/// - 词汇/结构/上下文信号分析
/// - 路由决策
/// - 领域检测
/// - 问题深度检测

#[test]
fn test_extract_signals_simple_query() {
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "find the file test.rs"}
        ]
    });

    let signals = extract_signals(&body);
    assert!(signals.lexical.has_simple_keywords);
    assert!(!signals.lexical.has_architecture_keywords);
}

#[test]
fn test_extract_signals_architecture() {
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "refactor the architecture design pattern"}
        ]
    });

    let signals = extract_signals(&body);
    assert!(signals.lexical.has_architecture_keywords);
}

#[test]
fn test_extract_signals_system_wide() {
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "refactor the entire project architecture"}
        ]
    });

    let signals = extract_signals(&body);
    assert!(signals.lexical.has_architecture_keywords);
    assert_eq!(signals.structural.impact_scope, ImpactScope::SystemWide);
}

#[test]
fn test_question_depth() {
    assert_eq!(detect_question_depth("why is this happening?"), QuestionDepth::Why);
    assert_eq!(detect_question_depth("how does this work?"), QuestionDepth::How);
    assert_eq!(detect_question_depth("what is this?"), QuestionDepth::What);
    assert_eq!(detect_question_depth("where is the file?"), QuestionDepth::Where);
    assert_eq!(detect_question_depth("display the code"), QuestionDepth::None);
}

#[test]
fn test_domain_detection() {
    assert_eq!(detect_domain("fix the security vulnerability"), DomainSpecificity::Security);
    assert_eq!(detect_domain("deploy to kubernetes"), DomainSpecificity::Infrastructure);
    assert_eq!(detect_domain("update the react component"), DomainSpecificity::Frontend);
    assert_eq!(detect_domain("fix the API endpoint"), DomainSpecificity::Backend);
    assert_eq!(detect_domain("update the readme"), DomainSpecificity::Generic);
}

#[test]
fn test_reversibility_assessment() {
    assert_eq!(assess_reversibility("migrate database"), Reversibility::Difficult);
    assert_eq!(assess_reversibility("refactor code"), Reversibility::Moderate);
    assert_eq!(assess_reversibility("add a function"), Reversibility::Easy);
}

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
            ..Default::default()
        },
        structural: StructuralSignals {
            impact_scope: ImpactScope::SystemWide,
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
fn test_route_simple_query() {
    let config = SmartRouterConfig::default();
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "find file location"}
        ]
    });

    let decision = route(&body, &config);
    assert_eq!(decision.tier, ComplexityTier::Low);
}

#[test]
fn test_route_complex_task() {
    let config = SmartRouterConfig::default();
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "refactor the entire project architecture and fix security vulnerabilities"}
        ]
    });

    let decision = route(&body, &config);
    assert_eq!(decision.tier, ComplexityTier::High);
}

#[test]
fn test_score_to_tier_conversion() {
    let config = SmartRouterConfig {
        low_threshold: 5,
        high_threshold: 10,
        ..Default::default()
    };

    assert_eq!(score_to_tier(3, &config), ComplexityTier::Low);
    assert_eq!(score_to_tier(7, &config), ComplexityTier::Medium);
    assert_eq!(score_to_tier(12, &config), ComplexityTier::High);
}

// =============================================================================
// 类型定义
//=============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuestionDepth {
    #[default]
    None,
    Where,
    What,
    How,
    Why,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImpactScope {
    #[default]
    Local,
    Module,
    SystemWide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reversibility {
    #[default]
    Easy,
    Moderate,
    Difficult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DomainSpecificity {
    #[default]
    Generic,
    Frontend,
    Backend,
    Infrastructure,
    Security,
}

#[derive(Debug, Default)]
pub struct LexicalSignals {
    pub word_count: usize,
    pub file_path_count: usize,
    pub code_block_count: usize,
    pub has_architecture_keywords: bool,
    pub has_debugging_keywords: bool,
    pub has_simple_keywords: bool,
    pub has_risk_keywords: bool,
    pub question_depth: QuestionDepth,
    pub has_implicit_requirements: bool,
}

#[derive(Debug, Default)]
pub struct StructuralSignals {
    pub estimated_subtasks: usize,
    pub cross_file_dependencies: bool,
    pub has_test_requirements: bool,
    pub domain_specificity: DomainSpecificity,
    pub requires_external_knowledge: bool,
    pub reversibility: Reversibility,
    pub impact_scope: ImpactScope,
}

#[derive(Debug, Default)]
pub struct ContextSignals {
    pub message_count: usize,
    pub tool_count: usize,
    pub has_thinking: bool,
    pub system_prompt_chars: usize,
    pub has_long_history: bool,
}

#[derive(Debug, Default)]
pub struct ComplexitySignals {
    pub lexical: LexicalSignals,
    pub structural: StructuralSignals,
    pub context: ContextSignals,
}

#[derive(Debug, Clone)]
pub struct SmartRouterConfig {
    pub enabled: bool,
    pub low_threshold: i32,
    pub high_threshold: i32,
}

impl Default for SmartRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            low_threshold: 4,
            high_threshold: 8,
        }
    }
}

#[derive(Debug)]
pub struct RoutingDecision {
    pub tier: ComplexityTier,
    pub confidence: f32,
    pub reasons: Vec<String>,
    pub matched_rule: Option<String>,
}

// =============================================================================
// 实现
//=============================================================================

fn extract_signals(body: &serde_json::Value) -> ComplexitySignals {
    let mut signals = ComplexitySignals::default();

    let messages = body.get("messages").and_then(|m| m.as_array());
    if let Some(messages) = messages {
        signals.context.message_count = messages.len();
        signals.context.has_long_history = messages.len() > 10;

        let recent_user_message = messages
            .iter()
            .rev()
            .find(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("user"));

        if let Some(msg) = recent_user_message {
            if let Some(content) = msg.get("content") {
                let user_text = extract_text_from_content(content);
                signals.lexical = extract_lexical_signals(&user_text);
                signals.structural = extract_structural_signals(&user_text);
            }
        }
    }

    signals
}

fn extract_text_from_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
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

fn extract_lexical_signals(text: &str) -> LexicalSignals {
    let mut signals = LexicalSignals::default();

    signals.word_count = text.split_whitespace().count();
    signals.file_path_count = count_file_paths(text);
    signals.code_block_count = text.matches("```").count() / 2;

    let text_lower = text.to_lowercase();
    signals.has_architecture_keywords =
        ["architecture", "refactor", "redesign", "架构", "重构"]
            .iter()
            .any(|k| text_lower.contains(k));
    signals.has_debugging_keywords =
        ["debug", "diagnose", "error", "调试", "排查"]
            .iter()
            .any(|k| text_lower.contains(k));
    signals.has_simple_keywords =
        ["find", "search", "show", "what is", "查找", "显示"]
            .iter()
            .any(|k| text_lower.contains(k));
    signals.has_risk_keywords =
        ["critical", "production", "security", "重要", "紧急"]
            .iter()
            .any(|k| text_lower.contains(k));

    signals.question_depth = detect_question_depth(text);
    signals
}

fn extract_structural_signals(text: &str) -> StructuralSignals {
    let mut signals = StructuralSignals::default();

    signals.estimated_subtasks = estimate_subtasks(text);
    signals.cross_file_dependencies = count_file_paths(text) >= 2;
    signals.has_test_requirements =
        ["test", "verify", "ensure", "测试", "验证"]
            .iter()
            .any(|k| text.to_lowercase().contains(k));
    signals.domain_specificity = detect_domain(text);
    signals.reversibility = assess_reversibility(text);
    signals.impact_scope = assess_impact_scope(text);

    signals
}

fn count_file_paths(text: &str) -> usize {
    let re = regex::Regex::new(r"[\w\-./]+\.\w{1,10}").unwrap();
    re.find_iter(text).count().min(text.len() / 5)
}

fn detect_question_depth(text: &str) -> QuestionDepth {
    let text_lower = text.to_lowercase();
    if text_lower.contains("why") || text_lower.contains("为什么") {
        return QuestionDepth::Why;
    }
    if text_lower.contains("how") || text_lower.contains("如何") {
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

fn estimate_subtasks(text: &str) -> usize {
    let bullet_count = text.matches("- ").count().min(5);
    let numbered = regex::Regex::new(r"\d+[.)]\s")
        .map(|re| re.find_iter(text).count())
        .unwrap_or(0)
        .min(5);
    let and_count = text.matches(" and ").count();

    (1 + bullet_count + numbered + and_count / 2).min(10)
}

fn detect_domain(text: &str) -> DomainSpecificity {
    let text_lower = text.to_lowercase();

    let security = ["security", "auth", "encryption", "安全"];
    if security.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Security;
    }

    let infra = ["docker", "kubernetes", "deploy", "容器", "部署"];
    if infra.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Infrastructure;
    }

    let frontend = ["react", "vue", "css", "html", "组件", "界面"];
    if frontend.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Frontend;
    }

    let backend = ["api", "database", "sql", "接口", "数据库"];
    if backend.iter().any(|p| text_lower.contains(p)) {
        return DomainSpecificity::Backend;
    }

    DomainSpecificity::Generic
}

fn assess_reversibility(text: &str) -> Reversibility {
    let text_lower = text.to_lowercase();

    let difficult = ["migrate", "production", "delete", "迁移", "删除"];
    if difficult.iter().any(|k| text_lower.contains(k)) {
        return Reversibility::Difficult;
    }

    let moderate = ["refactor", "rename", "重构", "重命名"];
    if moderate.iter().any(|k| text_lower.contains(k)) {
        return Reversibility::Moderate;
    }

    Reversibility::Easy
}

fn assess_impact_scope(text: &str) -> ImpactScope {
    let text_lower = text.to_lowercase();

    let system = ["entire", "all files", "whole system", "整个", "全部"];
    if system.iter().any(|k| text_lower.contains(k)) {
        return ImpactScope::SystemWide;
    }

    let module = ["module", "component", "service", "模块", "组件"];
    if module.iter().any(|k| text_lower.contains(k)) || count_file_paths(text) >= 3 {
        return ImpactScope::Module;
    }

    ImpactScope::Local
}

fn calculate_score(signals: &ComplexitySignals) -> i32 {
    let mut score = 0;

    if signals.lexical.has_architecture_keywords {
        score += 3;
    }
    if signals.lexical.has_debugging_keywords {
        score += 2;
    }
    if signals.lexical.has_simple_keywords {
        score -= 2;
    }
    if signals.lexical.word_count > 200 {
        score += 2;
    }
    if signals.lexical.file_path_count >= 2 {
        score += 1;
    }

    match signals.structural.impact_scope {
        ImpactScope::SystemWide => score += 3,
        ImpactScope::Module => score += 1,
        _ => {}
    }

    match signals.structural.reversibility {
        Reversibility::Difficult => score += 2,
        Reversibility::Moderate => score += 1,
        _ => {}
    }

    if signals.structural.domain_specificity == DomainSpecificity::Security {
        score += 2;
    }

    if signals.context.tool_count > 5 {
        score += 2;
    }
    if signals.context.has_thinking {
        score += 1;
    }

    score
}

fn score_to_tier(score: i32, config: &SmartRouterConfig) -> ComplexityTier {
    if score >= config.high_threshold {
        ComplexityTier::High
    } else if score < config.low_threshold {
        ComplexityTier::Low
    } else {
        ComplexityTier::Medium
    }
}

fn route(body: &serde_json::Value, config: &SmartRouterConfig) -> RoutingDecision {
    if !config.enabled {
        return RoutingDecision {
            tier: ComplexityTier::Medium,
            confidence: 1.0,
            reasons: vec!["智能路由已禁用".to_string()],
            matched_rule: None,
        };
    }

    let signals = extract_signals(body);
    let score = calculate_score(&signals);
    let tier = score_to_tier(score, config);

    let confidence = if tier == ComplexityTier::High {
        0.8 + (score - config.high_threshold) as f32 / 20.0
    } else if tier == ComplexityTier::Low {
        0.8 + (config.low_threshold - score) as f32 / 20.0
    } else {
        0.6
    };
    let confidence = confidence.min(0.99).max(0.5);

    let mut reasons = vec![format!("复杂度分数: {}", score)];
    if signals.lexical.has_architecture_keywords {
        reasons.push("包含架构关键词".to_string());
    }
    if signals.lexical.has_debugging_keywords {
        reasons.push("包含调试关键词".to_string());
    }

    RoutingDecision {
        tier,
        confidence,
        reasons,
        matched_rule: None,
    }
}
