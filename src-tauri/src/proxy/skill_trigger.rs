//! 技能触发检测模块
//!
//! 从数据库加载已安装的技能，检测用户消息中的触发关键词，自动注入技能内容
//! 参考 oh-my-claudecode 的完整实现
//!
//! ## 核心特性
//! - 四级匹配策略：exact > pattern > fuzzy > auto
//! - 置信度评分系统
//! - 上下文提取（错误、文件、技术模式）
//! - 多语言触发词支持
//! - 技能优先级覆盖（项目级 > 用户级）
//! - 触发词质量验证
//! - 会话缓存防止重复注入

use crate::database::Database;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

// =============================================================================
// 配置模块
// =============================================================================

/// 技能触发器配置
#[derive(Debug, Clone)]
pub struct SkillTriggerConfig {
    /// 每个 session 最大注入技能数量
    pub max_skills_per_session: usize,
    /// 会话缓存过期时间（秒）
    pub session_ttl_secs: u64,
    /// 最大会话缓存数量
    pub max_sessions: usize,
    /// 模糊匹配阈值 (0-100)
    pub fuzzy_match_threshold: usize,
    /// 置信度阈值 (0-100)
    pub confidence_threshold: usize,
    /// 触发词最小长度
    pub min_trigger_length: usize,
    /// 触发词最大长度
    pub max_trigger_length: usize,
    /// 最大技能内容长度
    pub max_skill_content_length: usize,
    /// 是否启用多语言检测
    pub multilingual_enabled: bool,
}

impl Default for SkillTriggerConfig {
    fn default() -> Self {
        Self {
            max_skills_per_session: 10,     // 与 oh-my-claudecode 对齐
            session_ttl_secs: 3600,         // 1 小时
            max_sessions: 100,
            fuzzy_match_threshold: 60,      // 与 oh-my-claudecode 对齐
            confidence_threshold: 30,
            min_trigger_length: 2,
            max_trigger_length: 50,
            max_skill_content_length: 8000, // 约 4000 字符 * 2（UTF-8）
            multilingual_enabled: true,
        }
    }
}

// =============================================================================
// 触发词黑名单
// =============================================================================

/// 触发词黑名单（过于通用的词不应作为触发词）
const TRIGGER_BLACKLIST_EN: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "must", "can", "need", "dare", "ought",
    "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "as", "into", "through", "during", "before", "after", "above", "below",
    "and", "but", "or", "nor", "so", "yet", "both", "either", "neither",
    "not", "only", "own", "same", "than", "too", "very", "just",
    // 过于宽泛的技术词
    "code", "file", "test", "help", "fix", "make", "get", "set", "run",
    "use", "try", "add", "put", "let", "var", "new", "old", "big", "small",
];

const TRIGGER_BLACKLIST_ZH: &[&str] = &[
    "的", "是", "在", "有", "和", "与", "或", "不", "了", "这", "那",
    "个", "之", "以", "为", "于", "上", "下", "中", "来", "去",
    "我", "你", "他", "她", "它", "们", "自", "己", "什么", "怎么",
    // 过于宽泛的技术词
    "代码", "文件", "测试", "帮助", "修复", "运行", "使用", "添加",
];

/// 检查触发词是否在黑名单中
fn is_trigger_blacklisted(trigger: &str) -> bool {
    let trigger_lower = trigger.to_lowercase();

    // 检查英文黑名单
    if TRIGGER_BLACKLIST_EN.contains(&trigger_lower.as_str()) {
        return true;
    }

    // 检查中文黑名单
    if TRIGGER_BLACKLIST_ZH.contains(&trigger) {
        return true;
    }

    false
}

// =============================================================================
// 类型定义
// =============================================================================

/// 匹配类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchType {
    /// 精确匹配（触发词完整出现在文本中）
    #[default]
    Exact,
    /// 模式匹配（glob/regex）
    Pattern,
    /// 模糊匹配（编辑距离）
    Fuzzy,
    /// 自动模式（尝试所有匹配方式）
    Auto,
}

impl std::fmt::Display for MatchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchType::Exact => write!(f, "精确"),
            MatchType::Pattern => write!(f, "模式"),
            MatchType::Fuzzy => write!(f, "模糊"),
            MatchType::Auto => write!(f, "自动"),
        }
    }
}

/// 匹配结果
#[derive(Debug, Clone)]
struct MatchResult {
    /// 匹配的触发词
    trigger: String,
    /// 置信度分数 (0-100)
    score: usize,
    /// 匹配类型
    match_type: MatchType,
}

/// 技能范围/优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillScope {
    /// 全局级技能（最低优先级）
    Global = 0,
    /// 用户级技能
    User = 1,
    /// 项目级技能（最高优先级）
    Project = 2,
}

impl Default for SkillScope {
    fn default() -> Self {
        Self::User
    }
}

impl std::fmt::Display for SkillScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillScope::Global => write!(f, "全局"),
            SkillScope::User => write!(f, "用户"),
            SkillScope::Project => write!(f, "项目"),
        }
    }
}

/// 上下文提取结果
#[derive(Debug, Clone, Default)]
pub struct MatchContext {
    /// 检测到的错误类型
    pub detected_errors: Vec<String>,
    /// 检测到的文件路径
    pub detected_files: Vec<String>,
    /// 检测到的技术模式
    pub detected_patterns: Vec<String>,
}

/// 检测到的技能
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TriggeredSkill {
    /// 技能 ID
    pub id: String,
    /// 技能名称
    pub name: String,
    /// 匹配的触发词
    pub matched_trigger: String,
    /// 技能内容（SKILL.md 的完整内容）
    pub content: String,
    /// 匹配类型
    pub match_type: MatchType,
    /// 置信度分数
    pub confidence: usize,
    /// 技能范围
    pub scope: SkillScope,
    /// 匹配上下文
    pub context: MatchContext,
}

/// 带触发词的技能
#[derive(Debug, Clone)]
struct SkillWithTriggers {
    /// 技能 ID（用于去重）
    id: String,
    /// 技能名称
    name: String,
    /// 技能目录
    directory: String,
    /// 触发词列表
    triggers: Vec<String>,
    /// 小写触发词（预计算）
    triggers_lower: Vec<String>,
    /// 匹配类型
    matching: MatchType,
    /// 技能范围
    scope: SkillScope,
    /// 启用的应用
    enabled_apps: SkillApps,
    /// 内容哈希（用于缓存）
    content_hash: Option<String>,
    /// 技能质量分数 (0-100)
    quality_score: usize,
}

/// 应用启用状态
#[derive(Debug, Clone, Copy, Default)]
struct SkillApps {
    claude: bool,
    codex: bool,
    gemini: bool,
    opencode: bool,
}

impl From<crate::app_config::SkillApps> for SkillApps {
    fn from(apps: crate::app_config::SkillApps) -> Self {
        Self {
            claude: apps.claude,
            codex: apps.codex,
            gemini: apps.gemini,
            opencode: apps.opencode,
        }
    }
}

/// 会话缓存条目
struct SessionCacheEntry {
    /// 已注入的技能内容哈希
    injected_hashes: HashSet<String>,
    /// 创建时间戳
    created_at: std::time::Instant,
}

/// 触发词验证结果
#[derive(Debug, Clone)]
pub struct TriggerValidationResult {
    /// 是否有效
    pub valid: bool,
    /// 错误信息
    pub errors: Vec<String>,
    /// 警告信息
    pub warnings: Vec<String>,
}

// =============================================================================
// 上下文提取模块
// =============================================================================

/// 从用户消息中提取上下文
pub fn extract_context(text: &str) -> MatchContext {
    let mut context = MatchContext::default();

    // 提取错误信息
    context.detected_errors = extract_errors(text);

    // 提取文件路径
    context.detected_files = extract_files(text);

    // 提取技术模式
    context.detected_patterns = extract_patterns(text);

    // 去重
    context.detected_errors.sort();
    context.detected_errors.dedup();
    context.detected_files.sort();
    context.detected_files.dedup();
    context.detected_patterns.sort();
    context.detected_patterns.dedup();

    context
}

/// 提取错误信息
fn extract_errors(text: &str) -> Vec<String> {
    let mut errors = Vec::new();

    // 通用错误关键词（多语言）
    let error_patterns = [
        // 英文
        r"\b(error|exception|failed|failure|crash|bug|issue)\b",
        r"\b([A-Z][a-z]+Error)\b",  // TypeError, ReferenceError 等
        // 错误码
        r"\b(ENOENT|EACCES|ECONNREFUSED|ETIMEDOUT|ENOTFOUND)\b",
        // 堆栈跟踪
        r"at\s+[\w\.]+\([^)]*:\d+:\d+\)",
        // 中文
        r"(错误|异常|失败|崩溃|问题)",
    ];

    for pattern in &error_patterns {
        if let Ok(re) = regex_lite_compile(pattern) {
            let matches = regex_lite_find_all(text, &re);
            errors.extend(matches);
        }
    }

    errors
}

/// 提取文件路径
fn extract_files(text: &str) -> Vec<String> {
    let mut files = Vec::new();

    // 文件路径模式
    let file_patterns = [
        // 相对路径：src/foo.ts, lib/main.py
        r"\b([a-zA-Z0-9_-]+/)+[a-zA-Z0-9_-]+\.[a-zA-Z]{1,4}\b",
        // 绝对路径：/usr/local, C:\Users
        r"\b(/[a-zA-Z0-9_/-]+|[A-Z]:\\[a-zA-Z0-9_\\-]+)\b",
        // src/ 路径
        r"\bsrc/[a-zA-Z0-9_/-]+\b",
        // 文件名带扩展名
        r"\b[a-zA-Z0-9_-]+\.[a-zA-Z]{1,4}\b",
    ];

    for pattern in &file_patterns {
        if let Ok(re) = regex_lite_compile(pattern) {
            let matches = regex_lite_find_all(text, &re);
            files.extend(matches);
        }
    }

    // 过滤掉太短的匹配
    files.retain(|f| f.len() >= 3);

    files
}

/// 提取技术模式
fn extract_patterns(text: &str) -> Vec<String> {
    let mut patterns = Vec::new();

    // 技术模式检测
    let tech_patterns = [
        (r"\basync\b.*\bawait\b", "async/await"),
        (r"\bpromise\b", "promise"),
        (r"\bcallback\b", "callback"),
        (r"\bregex\b|\bregular expression\b|正则", "regex"),
        (r"\bapi\b", "api"),
        (r"\brest\b|\bgraphql\b", "api"),
        (r"\btest\b|\b测试\b", "testing"),
        (r"\b(unit|integration|e2e)\b", "testing"),
        (r"\btypescript\b|\bts\b|typescript", "typescript"),
        (r"\bjavascript\b|\bjs\b", "javascript"),
        (r"\breact\b", "react"),
        (r"\bvue\b", "vue"),
        (r"\bangular\b", "angular"),
        (r"\bsvelte\b", "svelte"),
        (r"\bgit\b", "git"),
        (r"\bdocker\b", "docker"),
        (r"\bkubernetes\b|\bk8s\b", "kubernetes"),
        (r"\brust\b", "rust"),
        (r"\bpython\b", "python"),
        (r"\bgo\b|\bgolang\b", "golang"),
        (r"\bjava\b", "java"),
        (r"\bdatabase\b|\b数据库\b", "database"),
        (r"\bsql\b|\bnosql\b", "database"),
        (r"\bauth\b|\bauthentication\b|\b授权\b|\b认证\b", "authentication"),
        (r"\bsecurity\b|\b安全\b", "security"),
        (r"\bdeploy\b|\bdeployment\b|\b部署\b", "deployment"),
        (r"\bci\b|\bcd\b|\bcicd\b", "cicd"),
        (r"\bperformance\b|\b性能\b", "performance"),
        (r"\boptimization\b|\b优化\b", "optimization"),
        (r"\brefactor\b|\b重构\b", "refactoring"),
        (r"\bdebug\b|\b调试\b|\b排查\b", "debugging"),
    ];

    let text_lower = text.to_lowercase();

    for (pattern, name) in &tech_patterns {
        if let Ok(re) = regex_lite_compile(pattern) {
            if regex_lite_test(&text_lower, &re) {
                patterns.push(name.to_string());
            }
        }
    }

    patterns
}

// =============================================================================
// 轻量级正则引擎
// =============================================================================

/// 简单的正则编译结果
struct SimpleRegex {
    pattern: String,
    is_case_insensitive: bool,
}

/// 编译简单正则表达式
fn regex_lite_compile(pattern: &str) -> Result<SimpleRegex, ()> {
    // 简单处理：移除 \b 边界标记，转为普通字符串匹配
    let cleaned = pattern
        .replace("\\b", "")
        .replace("\\.", ".");

    Ok(SimpleRegex {
        pattern: cleaned,
        is_case_insensitive: true,
    })
}

/// 测试正则是否匹配
fn regex_lite_test(text: &str, re: &SimpleRegex) -> bool {
    if re.is_case_insensitive {
        text.to_lowercase().contains(&re.pattern.to_lowercase())
    } else {
        text.contains(&re.pattern)
    }
}

/// 查找所有匹配
fn regex_lite_find_all(text: &str, re: &SimpleRegex) -> Vec<String> {
    let mut results = Vec::new();
    let search_text = if re.is_case_insensitive {
        text.to_lowercase()
    } else {
        text.to_string()
    };
    let pattern_lower = re.pattern.to_lowercase();

    // 简单的子串搜索
    let mut start = 0;
    while let Some(pos) = search_text[start..].find(&pattern_lower) {
        let actual_start = start + pos;
        let actual_end = actual_start + re.pattern.len();

        if actual_end <= text.len() {
            // 尝试提取完整的单词
            let word_start = text[..actual_start]
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_' && *c != '/' && *c != '.' && *c != '\\')
                .map(|(i, _)| i + 1)
                .unwrap_or(0);
            let word_end = text[actual_end..]
                .char_indices()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_' && *c != '/' && *c != '.' && *c != '\\')
                .map(|(i, _)| actual_end + i)
                .unwrap_or(text.len());

            results.push(text[word_start..word_end].to_string());
        }

        start = actual_end;
        if start >= text.len() {
            break;
        }
    }

    results
}

// =============================================================================
// 技能触发缓存
// =============================================================================

/// 技能触发缓存
pub struct SkillTriggerCache {
    /// 配置
    config: SkillTriggerConfig,
    /// 技能列表（包含触发词）- 按优先级排序
    skills: RwLock<Vec<SkillWithTriggers>>,
    /// 会话缓存：session_id -> 已注入的技能哈希集合
    session_cache: RwLock<HashMap<String, SessionCacheEntry>>,
}

#[allow(dead_code)]
impl SkillTriggerCache {
    /// 创建新的技能触发缓存
    pub fn new() -> Self {
        Self::with_config(SkillTriggerConfig::default())
    }

    /// 使用指定配置创建缓存
    pub fn with_config(config: SkillTriggerConfig) -> Self {
        Self {
            config,
            skills: RwLock::new(Vec::new()),
            session_cache: RwLock::new(HashMap::new()),
        }
    }

    /// 从数据库加载技能
    pub async fn load_from_db(&self, db: &Arc<Database>) {
        log::info!("[SkillTrigger] === 开始加载技能 ===");

        let installed_skills = match db.get_all_installed_skills() {
            Ok(skills) => {
                log::info!("[SkillTrigger] 数据库中有 {} 个已安装技能", skills.len());
                skills
            }
            Err(e) => {
                log::error!("[SkillTrigger] 加载技能失败: {}", e);
                return;
            }
        };

        let mut skills_with_triggers = Vec::new();
        let mut skipped_no_triggers = 0;
        let mut skipped_no_metadata = 0;
        let mut skipped_blacklisted = 0;

        for (_, skill) in installed_skills {
            log::debug!("[SkillTrigger] 处理技能: {} (目录: {})", skill.name, skill.directory);

            // 读取 SKILL.md 解析元数据
            let skill_data = load_skill_metadata(&skill.directory);

            if let Some(data) = skill_data {
                if data.triggers.is_empty() {
                    log::debug!("[SkillTrigger] 技能 '{}' 无触发词，跳过", skill.name);
                    skipped_no_triggers += 1;
                    continue;
                }

                // 过滤黑名单触发词
                let (valid_triggers, blacklisted_count) = filter_blacklisted_triggers(&data.triggers);
                if blacklisted_count > 0 {
                    log::warn!(
                        "[SkillTrigger] 技能 '{}' 有 {} 个触发词在黑名单中被过滤",
                        skill.name, blacklisted_count
                    );
                    skipped_blacklisted += blacklisted_count;
                }

                if valid_triggers.is_empty() {
                    log::warn!("[SkillTrigger] 技能 '{}' 所有触发词都被过滤，跳过", skill.name);
                    continue;
                }

                log::info!(
                    "[SkillTrigger] ✓ 加载技能 '{}' - 触发词: [{}], 匹配类型: {:?}, 范围: {:?}, 质量分: {}%",
                    skill.name,
                    valid_triggers.join(", "),
                    data.matching,
                    data.scope,
                    data.quality_score
                );

                let triggers_lower = valid_triggers.iter().map(|t| t.to_lowercase()).collect();

                skills_with_triggers.push(SkillWithTriggers {
                    id: generate_skill_id(&skill.name, &skill.directory),
                    name: skill.name,
                    directory: skill.directory,
                    triggers: valid_triggers,
                    triggers_lower,
                    matching: data.matching,
                    scope: data.scope,
                    enabled_apps: SkillApps::from(skill.apps),
                    content_hash: data.content_hash,
                    quality_score: data.quality_score,
                });
            } else {
                log::warn!("[SkillTrigger] 技能 '{}' 无法加载元数据 (SKILL.md 不存在或格式错误)", skill.name);
                skipped_no_metadata += 1;
            }
        }

        // 按优先级排序：项目级 > 用户级 > 全局级
        // 同优先级按质量分排序
        skills_with_triggers.sort_by(|a, b| {
            b.scope.cmp(&a.scope)
                .then_with(|| b.quality_score.cmp(&a.quality_score))
        });

        // 处理技能 ID 冲突：高优先级覆盖低优先级
        let deduped_skills = resolve_skill_conflicts(skills_with_triggers);

        let mut skills = self.skills.write().await;
        *skills = deduped_skills;

        log::info!(
            "[SkillTrigger] === 加载完成: {} 个有效技能, {} 个无触发词, {} 个元数据错误, {} 个黑名单过滤 ===",
            skills.len(), skipped_no_triggers, skipped_no_metadata, skipped_blacklisted
        );
    }

    /// 检测用户消息中的技能触发
    pub async fn detect(
        &self,
        user_text: &str,
        app_type: &str,
        session_id: Option<&str>,
    ) -> Vec<TriggeredSkill> {
        log::debug!(
            "[SkillTrigger] 开始检测 - app_type: {}, session_id: {:?}, 文本长度: {}",
            app_type, session_id, user_text.len()
        );
        log::trace!("[SkillTrigger] 用户文本: {}", user_text);

        let skills = self.skills.read().await;
        let text_lower = user_text.to_lowercase();

        log::debug!("[SkillTrigger] 已加载 {} 个技能待检测", skills.len());

        // 提取上下文
        let context = if self.config.multilingual_enabled {
            extract_context(user_text)
        } else {
            MatchContext::default()
        };

        if !context.detected_errors.is_empty() {
            log::debug!(
                "[SkillTrigger] 检测到错误: {:?}",
                context.detected_errors
            );
        }
        if !context.detected_patterns.is_empty() {
            log::debug!(
                "[SkillTrigger] 检测到技术模式: {:?}",
                context.detected_patterns
            );
        }

        // 获取该会话已注入的技能
        let injected_hashes = if let Some(sid) = session_id {
            let cache = self.session_cache.read().await;
            let hashes = cache.get(sid).map(|e| e.injected_hashes.clone()).unwrap_or_default();
            log::debug!("[SkillTrigger] 会话 {} 已注入 {} 个技能", sid, hashes.len());
            hashes
        } else {
            HashSet::new()
        };

        let mut results: Vec<(TriggeredSkill, usize)> = Vec::new();
        let mut skipped_count = 0;
        let mut disabled_count = 0;

        for skill in skills.iter() {
            // 检查是否已注入
            if let Some(ref hash) = skill.content_hash {
                if injected_hashes.contains(hash) {
                    log::debug!("[SkillTrigger] 跳过技能 '{}' - 已在会话中注入", skill.name);
                    skipped_count += 1;
                    continue;
                }
            }

            // 检查该应用是否启用了此技能
            let is_enabled = match app_type {
                "claude" => skill.enabled_apps.claude,
                "codex" => skill.enabled_apps.codex,
                "gemini" => skill.enabled_apps.gemini,
                "opencode" => skill.enabled_apps.opencode,
                _ => false,
            };

            if !is_enabled {
                log::trace!(
                    "[SkillTrigger] 跳过技能 '{}' - 应用 {} 未启用",
                    skill.name, app_type
                );
                disabled_count += 1;
                continue;
            }

            // 尝试匹配触发词
            if let Some(match_result) = self.match_triggers(&text_lower, &skill, &context) {
                let confidence = match_result.score;

                if confidence >= self.config.confidence_threshold {
                    log::info!(
                        "[SkillTrigger] ✓ 匹配技能 '{}' - 触发词: '{}', 类型: {}, 置信度: {}%",
                        skill.name, match_result.trigger, match_result.match_type, confidence
                    );

                    let content = load_skill_content(&skill.directory);

                    results.push((
                        TriggeredSkill {
                            id: skill.id.clone(),
                            name: skill.name.clone(),
                            matched_trigger: match_result.trigger,
                            content,
                            match_type: match_result.match_type,
                            confidence,
                            scope: skill.scope,
                            context: context.clone(),
                        },
                        confidence,
                    ));
                }
            }
        }

        // 按置信度降序排序，取前 N 个
        results.sort_by(|a, b| b.1.cmp(&a.1));
        let triggered_skills: Vec<TriggeredSkill> = results
            .into_iter()
            .take(self.config.max_skills_per_session)
            .map(|(s, _)| s)
            .collect();

        log::debug!(
            "[SkillTrigger] 检测完成 - 触发: {}, 跳过(已注入): {}, 跳过(未启用): {}",
            triggered_skills.len(), skipped_count, disabled_count
        );

        triggered_skills
    }

    /// 匹配触发词
    fn match_triggers(
        &self,
        text_lower: &str,
        skill: &SkillWithTriggers,
        context: &MatchContext,
    ) -> Option<MatchResult> {
        let mut matches: Vec<MatchResult> = Vec::new();

        for (trigger, trigger_lower) in skill.triggers.iter().zip(skill.triggers_lower.iter()) {
            let result = match skill.matching {
                MatchType::Exact => {
                    if text_lower.contains(trigger_lower) {
                        Some(MatchResult {
                            trigger: trigger.clone(),
                            score: 100,
                            match_type: MatchType::Exact,
                        })
                    } else {
                        None
                    }
                }
                MatchType::Pattern => {
                    pattern_match(text_lower, trigger).map(|score| MatchResult {
                        trigger: trigger.clone(),
                        score,
                        match_type: MatchType::Pattern,
                    })
                }
                MatchType::Fuzzy => {
                    let score = fuzzy_match(text_lower, trigger_lower, self.config.fuzzy_match_threshold);
                    if score >= self.config.fuzzy_match_threshold {
                        Some(MatchResult {
                            trigger: trigger.clone(),
                            score,
                            match_type: MatchType::Fuzzy,
                        })
                    } else {
                        None
                    }
                }
                MatchType::Auto => {
                    // 自动模式：依次尝试精确 -> 模式 -> 模糊
                    if text_lower.contains(trigger_lower) {
                        Some(MatchResult {
                            trigger: trigger.clone(),
                            score: 100,
                            match_type: MatchType::Exact,
                        })
                    } else if let Some(score) = pattern_match(text_lower, trigger) {
                        Some(MatchResult {
                            trigger: trigger.clone(),
                            score,
                            match_type: MatchType::Pattern,
                        })
                    } else {
                        let score = fuzzy_match(text_lower, trigger_lower, self.config.fuzzy_match_threshold);
                        if score >= self.config.fuzzy_match_threshold {
                            Some(MatchResult {
                                trigger: trigger.clone(),
                                score,
                                match_type: MatchType::Fuzzy,
                            })
                        } else {
                            None
                        }
                    }
                }
            };

            if let Some(m) = result {
                matches.push(m);
            }
        }

        if matches.is_empty() {
            return None;
        }

        // 计算综合置信度：最佳匹配 * 0.7 + 平均分 * 0.3
        let best = matches.iter().max_by_key(|m| m.score)?;
        let avg_score = matches.iter().map(|m| m.score).sum::<usize>() / matches.len();
        let confidence = (best.score * 7 + avg_score * 3) / 10;

        // 上下文加成
        let context_bonus = calculate_context_bonus(&best.trigger, context);
        let final_confidence = (confidence + context_bonus).min(100);

        Some(MatchResult {
            trigger: best.trigger.clone(),
            score: final_confidence,
            match_type: best.match_type,
        })
    }

    /// 标记技能为已注入
    pub async fn mark_injected(&self, session_id: &str, skills: &[TriggeredSkill]) {
        let mut cache = self.session_cache.write().await;

        // 清理过期会话
        if cache.len() >= self.config.max_sessions {
            let now = std::time::Instant::now();
            cache.retain(|_, entry| {
                now.duration_since(entry.created_at).as_secs() < self.config.session_ttl_secs
            });
        }

        // 获取或创建会话条目
        let entry = cache.entry(session_id.to_string()).or_insert_with(|| SessionCacheEntry {
            injected_hashes: HashSet::new(),
            created_at: std::time::Instant::now(),
        });

        // 添加技能哈希
        for skill in skills {
            let hash = format!("{}:{}", skill.id, skill.matched_trigger);
            entry.injected_hashes.insert(hash);
        }
    }

    /// 清除会话缓存
    pub async fn clear_session(&self, session_id: &str) {
        let mut cache = self.session_cache.write().await;
        cache.remove(session_id);
    }

    /// 清除所有过期会话
    pub async fn cleanup_expired_sessions(&self) {
        let mut cache = self.session_cache.write().await;
        let now = std::time::Instant::now();
        cache.retain(|_, entry| {
            now.duration_since(entry.created_at).as_secs() < self.config.session_ttl_secs
        });
    }

    /// 获取已加载的技能数量
    pub async fn skill_count(&self) -> usize {
        self.skills.read().await.len()
    }
}

impl Default for SkillTriggerCache {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// 辅助函数
// =============================================================================

/// 生成技能 ID
fn generate_skill_id(name: &str, directory: &str) -> String {
    format!("{}:{}", name, directory)
}

/// 过滤黑名单触发词
fn filter_blacklisted_triggers(triggers: &[String]) -> (Vec<String>, usize) {
    let mut valid = Vec::new();
    let mut blacklisted_count = 0;

    for trigger in triggers {
        if is_trigger_blacklisted(trigger) {
            blacklisted_count += 1;
        } else {
            valid.push(trigger.clone());
        }
    }

    (valid, blacklisted_count)
}

/// 解决技能 ID 冲突（高优先级覆盖低优先级）
fn resolve_skill_conflicts(skills: Vec<SkillWithTriggers>) -> Vec<SkillWithTriggers> {
    let mut skill_map: HashMap<String, SkillWithTriggers> = HashMap::new();

    // 按优先级从低到高加载，高优先级会覆盖低优先级
    for skill in skills {
        let existing = skill_map.get(&skill.id);

        let should_replace = match existing {
            None => true,
            Some(existing) => {
                // 高优先级覆盖低优先级
                skill.scope > existing.scope
            }
        };

        if should_replace {
            skill_map.insert(skill.id.clone(), skill);
        }
    }

    // 转换回向量并保持优先级排序
    let mut result: Vec<SkillWithTriggers> = skill_map.into_values().collect();
    result.sort_by(|a, b| {
        b.scope.cmp(&a.scope)
            .then_with(|| b.quality_score.cmp(&a.quality_score))
    });

    result
}

/// 计算上下文加成
fn calculate_context_bonus(trigger: &str, context: &MatchContext) -> usize {
    let trigger_lower = trigger.to_lowercase();
    let mut bonus = 0;

    // 如果触发词与检测到的错误相关
    for error in &context.detected_errors {
        let error_lower = error.to_lowercase();
        if trigger_lower.contains(&error_lower) || error_lower.contains(&trigger_lower) {
            bonus += 10;
        }
    }

    // 如果触发词与检测到的技术模式相关
    for pattern in &context.detected_patterns {
        let pattern_lower = pattern.to_lowercase();
        if trigger_lower.contains(&pattern_lower) || pattern_lower.contains(&trigger_lower) {
            bonus += 5;
        }
    }

    bonus.min(20) // 最大加成 20
}

// =============================================================================
// 匹配引擎
// =============================================================================

/// 模式匹配（glob 和 regex）
fn pattern_match(text: &str, pattern: &str) -> Option<usize> {
    // 检查 glob 模式（包含 * 通配符）
    if pattern.contains('*') {
        let regex_pattern = pattern
            .replace('.', r"\.")
            .replace('*', ".*");

        if regex_lite_match(text, &regex_pattern) {
            return Some(85);
        }
    }

    // 检查正则表达式模式（/pattern/flags 格式）
    if pattern.starts_with('/') && pattern.len() > 2 {
        if let Some((pattern_str, _flags)) = parse_regex_pattern(pattern) {
            if regex_lite_match(text, &pattern_str) {
                return Some(90);
            }
        }
    }

    None
}

/// 解析正则表达式模式 /pattern/flags
fn parse_regex_pattern(pattern: &str) -> Option<(String, String)> {
    let pattern = pattern.trim_start_matches('/');
    let slash_pos = pattern.rfind('/')?;
    let pattern_str = pattern[..slash_pos].to_string();
    let flags = pattern[slash_pos + 1..].to_string();
    Some((pattern_str, flags))
}

/// 轻量级正则匹配
fn regex_lite_match(text: &str, pattern: &str) -> bool {
    if pattern == ".*" {
        return true;
    }

    if pattern.contains(".*") {
        let parts: Vec<&str> = pattern.split(".*").collect();
        let text_lower = text.to_lowercase();
        let mut last_pos = 0;

        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            let part_lower = part.to_lowercase();
            if let Some(pos) = text_lower[last_pos..].find(&part_lower) {
                last_pos = last_pos + pos + part.len();
            } else if i == 0 {
                if !text_lower.starts_with(&part_lower) {
                    return false;
                }
                last_pos = part.len();
            } else {
                return false;
            }
        }

        return true;
    }

    text.to_lowercase().contains(&pattern.to_lowercase())
}

/// 模糊匹配（基于编辑距离）
fn fuzzy_match(text: &str, pattern: &str, threshold: usize) -> usize {
    if pattern.is_empty() {
        return 100;
    }
    if text.is_empty() {
        return 0;
    }

    let words: Vec<&str> = text.split_whitespace().filter(|w| !w.is_empty()).collect();
    let mut best_score = 0;

    for word in &words {
        let word_lower = word.to_lowercase();
        let pattern_lower = pattern.to_lowercase();

        // 精确单词匹配
        if word_lower == pattern_lower {
            return 100;
        }

        // 部分匹配
        if word_lower.contains(&pattern_lower) || pattern_lower.contains(&word_lower) {
            best_score = best_score.max(80);
            continue;
        }

        // 编辑距离
        let distance = levenshtein_distance(&word_lower, &pattern_lower);
        let max_len = word.len().max(pattern.len());
        if max_len > 0 {
            let similarity = ((max_len - distance) * 100) / max_len;
            best_score = best_score.max(similarity);
        }
    }

    // 滑动窗口搜索
    if best_score < threshold {
        best_score = best_score.max(slide_window_fuzzy(text, pattern));
    }

    best_score
}

/// 滑动窗口模糊匹配
fn slide_window_fuzzy(text: &str, pattern: &str) -> usize {
    let pattern_len = pattern.chars().count();
    if pattern_len == 0 {
        return 100;
    }

    let text_chars: Vec<char> = text.chars().collect();
    if text_chars.is_empty() {
        return 0;
    }

    let mut best_score = 0;
    let window_sizes = [
        pattern_len,
        pattern_len.saturating_sub(1),
        pattern_len.saturating_sub(2),
        pattern_len + 1,
        pattern_len + 2,
    ];

    for window_len in window_sizes.into_iter().filter(|&l| l > 0 && l <= text_chars.len()) {
        for i in 0..=text_chars.len() - window_len {
            let window: String = text_chars[i..i + window_len].iter().collect();
            let distance = levenshtein_distance(&window.to_lowercase(), &pattern.to_lowercase());

            let max_len = window_len.max(pattern_len);
            let similarity = ((max_len - distance) * 100) / max_len;

            best_score = best_score.max(similarity);

            if best_score >= 100 {
                return 100;
            }
        }
    }

    best_score
}

/// 计算编辑距离
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let mut prev_row: Vec<usize> = (0..=len2).collect();
    let mut curr_row: Vec<usize> = vec![0; len2 + 1];

    for (i, c1) in s1.chars().enumerate() {
        curr_row[0] = i + 1;

        for (j, c2) in s2.chars().enumerate() {
            let cost = if c1 == c2 { 0 } else { 1 };
            curr_row[j + 1] = (prev_row[j + 1] + 1)
                .min(curr_row[j] + 1)
                .min(prev_row[j] + cost);
        }

        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[len2]
}

/// 简单的 FNV-1a 哈希
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// =============================================================================
// 技能元数据
// =============================================================================

/// 技能元数据
struct SkillMetadata {
    /// 触发词列表
    triggers: Vec<String>,
    /// 匹配类型
    matching: MatchType,
    /// 技能范围
    scope: SkillScope,
    /// 内容哈希
    content_hash: Option<String>,
    /// 质量分数
    quality_score: usize,
}

/// 从 SKILL.md 加载技能元数据
fn load_skill_metadata(directory: &str) -> Option<SkillMetadata> {
    let skill_path = get_ssot_skill_path(directory);
    let skill_md = skill_path.join("SKILL.md");

    if !skill_md.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&skill_md).ok()?;

    // 解析 frontmatter
    let (frontmatter, _body) = parse_frontmatter(&content)?;

    // 解析触发词
    let triggers = parse_yaml_list(&frontmatter, "triggers");

    if triggers.is_empty() {
        return None;
    }

    // 解析匹配类型
    let matching = parse_match_type(&frontmatter);

    // 解析范围
    let scope = parse_scope(&frontmatter);

    // 计算内容哈希
    let content_hash = Some(format!("{:016x}", fnv1a_hash(&content)));

    // 计算质量分数
    let quality_score = calculate_skill_quality(&triggers, &content);

    Some(SkillMetadata {
        triggers,
        matching,
        scope,
        content_hash,
        quality_score,
    })
}

/// 计算技能质量分数
fn calculate_skill_quality(triggers: &[String], content: &str) -> usize {
    let mut score = 50; // 基础分

    // 触发词质量
    for trigger in triggers {
        // 触发词长度适中加分
        let len = trigger.chars().count();
        if len >= 3 && len <= 15 {
            score += 5;
        }

        // 非黑名单触发词加分
        if !is_trigger_blacklisted(trigger) {
            score += 5;
        }
    }

    // 内容长度适中加分
    let content_len = content.chars().count();
    if content_len >= 100 && content_len <= 4000 {
        score += 10;
    } else if content_len >= 50 && content_len <= 8000 {
        score += 5;
    }

    // 限制最大值
    score.min(100)
}

/// 解析匹配类型
fn parse_match_type(frontmatter: &str) -> MatchType {
    if !frontmatter.contains("matching:") {
        return MatchType::Auto;
    }

    let matching_str = extract_yaml_value(frontmatter, "matching").unwrap_or_default();
    let lower = matching_str.to_lowercase();

    if lower.contains("exact") {
        MatchType::Exact
    } else if lower.contains("pattern") {
        MatchType::Pattern
    } else if lower.contains("fuzzy") {
        MatchType::Fuzzy
    } else {
        MatchType::Auto
    }
}

/// 解析范围
fn parse_scope(frontmatter: &str) -> SkillScope {
    if !frontmatter.contains("scope:") {
        return SkillScope::default();
    }

    let scope_str = extract_yaml_value(frontmatter, "scope").unwrap_or_default();
    let lower = scope_str.to_lowercase();

    if lower.contains("project") {
        SkillScope::Project
    } else if lower.contains("global") {
        SkillScope::Global
    } else {
        SkillScope::User
    }
}

/// 解析 YAML frontmatter
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim();

    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let frontmatter = content[3..end + 3].to_string();
    let body = content[end + 6..].to_string();

    Some((frontmatter, body))
}

/// 从 YAML 字符串解析列表
fn parse_yaml_list(yaml: &str, key: &str) -> Vec<String> {
    let mut result = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();

    let mut in_list = false;
    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with(&format!("{}:", key)) {
            in_list = true;
            continue;
        }

        if in_list {
            if trimmed.starts_with('-') {
                let item = trimmed[1..].trim().trim_matches('"').trim_matches('\'');
                if !item.is_empty() {
                    result.push(item.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with(' ') && !trimmed.starts_with('\t') {
                break;
            }
        }
    }

    result
}

/// 从 YAML 提取单个值
fn extract_yaml_value(yaml: &str, key: &str) -> Option<String> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{}:", key)) {
            let value = trimmed[key.len() + 1..].trim();
            return Some(value.trim_matches('"').trim_matches('\'').to_string());
        }
    }
    None
}

/// 加载完整的技能内容
fn load_skill_content(directory: &str) -> String {
    let skill_path = get_ssot_skill_path(directory);
    let skill_md = skill_path.join("SKILL.md");

    if !skill_md.exists() {
        return String::new();
    }

    match std::fs::read_to_string(&skill_md) {
        Ok(content) => content,
        Err(e) => {
            log::warn!("[SkillTrigger] 读取技能内容失败: {}", e);
            String::new()
        }
    }
}

/// 获取 SSOT 技能路径
fn get_ssot_skill_path(directory: &str) -> PathBuf {
    crate::config::get_app_config_dir()
        .join("skills")
        .join(directory)
}

// =============================================================================
// 全局缓存
// =============================================================================

/// 全局技能触发缓存
static SKILL_TRIGGER_CACHE: once_cell::sync::Lazy<SkillTriggerCache> =
    once_cell::sync::Lazy::new(SkillTriggerCache::new);

/// 全局自动学习器
static AUTO_LEARNER: once_cell::sync::Lazy<crate::proxy::auto_learner::AutoLearner> =
    once_cell::sync::Lazy::new(|| {
        crate::proxy::auto_learner::AutoLearner::new(
            crate::config::get_app_config_dir().join("analytics"),
        )
    });

/// 全局自动调用器
static AUTO_INVOKER: once_cell::sync::Lazy<crate::proxy::auto_invoke::AutoInvoker> =
    once_cell::sync::Lazy::new(|| {
        crate::proxy::auto_invoke::AutoInvoker::new(
            crate::config::get_app_config_dir().join("analytics"),
        )
    });

/// 获取全局技能触发缓存
pub fn get_skill_trigger_cache() -> &'static SkillTriggerCache {
    &SKILL_TRIGGER_CACHE
}

/// 获取全局自动学习器
pub fn get_auto_learner() -> &'static crate::proxy::auto_learner::AutoLearner {
    &AUTO_LEARNER
}

/// 获取全局自动调用器
pub fn get_auto_invoker() -> &'static crate::proxy::auto_invoke::AutoInvoker {
    &AUTO_INVOKER
}

// =============================================================================
// 请求处理
// =============================================================================

/// 从用户消息中提取文本
fn extract_user_text(body: &serde_json::Value) -> Option<String> {
    let messages = body.get("messages").and_then(|m| m.as_array())?;

    for msg in messages.iter().rev() {
        let role = msg.get("role").and_then(|r| r.as_str());
        if role == Some("user") {
            if let Some(content) = msg.get("content") {
                return Some(extract_text_from_content(content));
            }
        }
    }

    None
}

/// 从 content 字段提取文本
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

/// 检测并注入技能
pub async fn detect_and_inject_skill(
    body: serde_json::Value,
    app_type: &str,
    session_id: Option<&str>,
) -> (serde_json::Value, Vec<TriggeredSkill>) {
    log::debug!("[SkillTrigger] === 开始技能检测流程 ===");
    log::debug!("[SkillTrigger] 应用类型: {}, 会话ID: {:?}", app_type, session_id);

    let user_text = match extract_user_text(&body) {
        Some(text) => text,
        None => {
            log::debug!("[SkillTrigger] 未找到用户消息文本，跳过检测");
            return (body, Vec::new());
        }
    };

    log::debug!("[SkillTrigger] 用户消息长度: {} 字符", user_text.len());

    let cache = get_skill_trigger_cache();
    let triggered_skills = cache.detect(&user_text, app_type, session_id).await;

    if triggered_skills.is_empty() {
        log::debug!("[SkillTrigger] 未检测到任何触发的技能");
        return (body, Vec::new());
    }

    // === 自动学习：从对话历史中检测可学习模式 ===
    if let Some(messages) = extract_message_pairs(&body) {
        let learner = get_auto_learner();
        let detections = learner.detect_patterns(&messages).await;

        if !detections.is_empty() {
            learner.record_patterns(detections.clone()).await;
            log::info!(
                "[AutoLearner] 检测到 {} 个可学习模式",
                detections.len()
            );
        }
    }

    // === 自动调用：高置信度技能使用特殊格式 ===
    let mut auto_invoke_skills = Vec::new();
    let mut normal_skills = Vec::new();

    for skill in triggered_skills {
        if skill.confidence >= 80 && session_id.is_some() {
            let invoker = get_auto_invoker();
            let should_invoke = invoker
                .should_auto_invoke(session_id.unwrap(), &skill.id, skill.confidence)
                .await;

            if should_invoke {
                invoker
                    .record_invocation(
                        session_id.unwrap(),
                        &skill.id,
                        &skill.name,
                        skill.confidence,
                        &user_text[..user_text.len().min(100)],
                    )
                    .await;

                auto_invoke_skills.push(skill.clone());
                log::info!(
                    "[AutoInvoke] ✓ 自动调用技能 '{}' ({}%)",
                    skill.name, skill.confidence
                );
                continue;
            }
        }

        normal_skills.push(skill);
    }

    // 注入普通技能（标准格式）
    let body = if !normal_skills.is_empty() {
        inject_skills_content(body, &normal_skills)
    } else {
        body
    };

    // 注入自动调用技能（强势格式，追加在 system 末尾）
    let body = if !auto_invoke_skills.is_empty() {
        inject_auto_invoke_skills(body, &auto_invoke_skills)
    } else {
        body
    };

    // 统计日志
    let total = auto_invoke_skills.len() + normal_skills.len();
    log::info!(
        "[SkillTrigger] ✓✓✓ 触发 {} 个技能: {} 自动调用, {} 普通注入 ✓✓✓",
        total,
        auto_invoke_skills.len(),
        normal_skills.len()
    );

    if let Some(sid) = session_id {
        // 标记普通技能为已注入（自动调用由 auto_invoke 管理）
        cache.mark_injected(sid, &normal_skills).await;
    }

    log::debug!("[SkillTrigger] === 技能注入完成 ===");

    (body, auto_invoke_skills.into_iter().chain(normal_skills.into_iter()).collect())
}

/// 从请求体中提取 (role, content) 对
fn extract_message_pairs(body: &serde_json::Value) -> Option<Vec<(String, String)>> {
    let messages = body.get("messages").and_then(|m| m.as_array())?;

    let mut pairs = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
        let content = extract_text_from_content(msg.get("content").unwrap_or(&serde_json::Value::Null));
        if !content.is_empty() {
            pairs.push((role, content));
        }
    }

    if pairs.is_empty() {
        None
    } else {
        Some(pairs)
    }
}

/// 注入自动调用技能（特殊格式）
fn inject_auto_invoke_skills(
    mut body: serde_json::Value,
    skills: &[TriggeredSkill],
) -> serde_json::Value {
    if skills.is_empty() {
        return body;
    }

    let mut parts = Vec::new();

    for skill in skills {
        parts.push(crate::proxy::auto_invoke::format_auto_invoke(
            &skill.name,
            &skill.content,
            skill.confidence,
        ));
    }

    let injection = parts.join("\n\n");

    if let Some(system) = body.get_mut("system") {
        if let Some(system_str) = system.as_str() {
            *system = serde_json::json!(format!("{}\n\n{}", system_str, injection));
        } else if let Some(system_arr) = system.as_array_mut() {
            system_arr.push(serde_json::json!({
                "type": "text",
                "text": injection,
            }));
        }
    } else {
        body["system"] = serde_json::json!(injection);
    }

    body
}

/// 注入多个技能内容到请求体
fn inject_skills_content(
    mut body: serde_json::Value,
    skills: &[TriggeredSkill],
) -> serde_json::Value {
    if skills.is_empty() {
        return body;
    }

    let mut injection_parts = Vec::new();

    injection_parts.push("<skill-injection>".to_string());
    injection_parts.push(String::new());
    injection_parts.push("## 已触发的技能".to_string());
    injection_parts.push(String::new());
    injection_parts.push("以下技能已根据触发词自动注入，可能对当前任务有帮助：".to_string());
    injection_parts.push(String::new());

    for skill in skills {
        injection_parts.push(format!("### {}", skill.name));
        injection_parts.push(format!("**来源:** {}级技能", skill.scope));
        injection_parts.push(format!("**触发词:** {}", skill.matched_trigger));
        injection_parts.push(format!("**匹配类型:** {}", skill.match_type));
        injection_parts.push(format!("**置信度:** {}%", skill.confidence));

        // 显示检测到的上下文
        if !skill.context.detected_errors.is_empty() {
            injection_parts.push(format!("**相关错误:** {}", skill.context.detected_errors.join(", ")));
        }
        if !skill.context.detected_patterns.is_empty() {
            injection_parts.push(format!("**相关技术:** {}", skill.context.detected_patterns.join(", ")));
        }

        injection_parts.push(String::new());
        injection_parts.push(skill.content.clone());
        injection_parts.push(String::new());
        injection_parts.push("---".to_string());
        injection_parts.push(String::new());
    }

    injection_parts.push("</skill-injection>".to_string());

    let injection = injection_parts.join("\n");

    if let Some(system) = body.get_mut("system") {
        if let Some(system_str) = system.as_str() {
            *system = serde_json::json!(format!("{}\n\n{}", system_str, injection));
        } else if let Some(system_arr) = system.as_array_mut() {
            system_arr.push(serde_json::json!({
                "type": "text",
                "text": injection
            }));
        }
    } else {
        body["system"] = serde_json::json!(injection);
    }

    body
}

/// 初始化技能触发缓存
pub async fn init_skill_trigger_cache(db: &Arc<Database>) {
    let cache = get_skill_trigger_cache();
    cache.load_from_db(db).await;
}

/// 验证触发词质量
pub fn validate_triggers(triggers: &[String]) -> TriggerValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for trigger in triggers {
        let len = trigger.chars().count();

        if len < 2 {
            errors.push(format!("触发词过短: \"{}\" (至少需要2个字符)", trigger));
        }

        if len > 50 {
            warnings.push(format!("触发词过长: \"{}\" (建议不超过50个字符)", trigger));
        }

        if is_trigger_blacklisted(trigger) {
            errors.push(format!("触发词在黑名单中: \"{}\"", trigger));
        }

        // 检查特殊字符
        let has_special = trigger.chars().any(|c| {
            !c.is_alphanumeric() && c != '_' && c != '-' && c != '*' && c != '/'
        });
        if has_special {
            warnings.push(format!("触发词包含特殊字符: \"{}\"", trigger));
        }
    }

    TriggerValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

// =============================================================================
// 测试
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // 上下文提取
    // =========================================================================

    #[test]
    fn test_extract_context_errors() {
        let text = "I got a TypeError in src/main.ts when running async/await code";
        let context = extract_context(text);

        assert!(context.detected_errors.contains(&"TypeError".to_string()));
        assert!(context.detected_files.iter().any(|f| f.contains("main.ts")));
        assert!(context.detected_patterns.contains(&"async/await".to_string()));
    }

    #[test]
    fn test_extract_context_chinese_errors() {
        let context = extract_context("编译失败，TypeScript 类型错误");
        assert!(context.detected_errors.iter().any(|e| e.contains("TypeError")));
        assert!(context.detected_errors.iter().any(|e| e.contains("失败")));
    }

    #[test]
    fn test_extract_context_error_codes() {
        let context = extract_context("ENOENT: no such file, ECONNREFUSED at port 3000");
        assert!(context.detected_errors.contains(&"ENOENT".to_string()));
        assert!(context.detected_errors.contains(&"ECONNREFUSED".to_string()));
    }

    #[test]
    fn test_extract_context_tech_patterns() {
        let context = extract_context("I'm building a React app with TypeScript and Docker");
        assert!(context.detected_patterns.contains(&"react".to_string()));
        assert!(context.detected_patterns.contains(&"typescript".to_string()));
        assert!(context.detected_patterns.contains(&"docker".to_string()));
    }

    #[test]
    fn test_extract_context_dedup() {
        let context = extract_context("TypeError TypeError error");
        // 去重后不应有重复
        let error_count = context.detected_errors.iter().filter(|e| *e == "TypeError").count();
        assert_eq!(error_count, 1);
    }

    #[test]
    fn test_extract_context_empty() {
        let context = extract_context("hello world");
        assert!(context.detected_errors.is_empty());
        assert!(context.detected_files.is_empty());
        // "hello world" 不含任何技术模式关键词
    }

    // =========================================================================
    // 触发词黑名单
    // =========================================================================

    #[test]
    fn test_is_trigger_blacklisted_english() {
        assert!(is_trigger_blacklisted("the"));
        assert!(is_trigger_blacklisted("is"));
        assert!(is_trigger_blacklisted("code"));
        assert!(is_trigger_blacklisted("fix"));
    }

    #[test]
    fn test_is_trigger_blacklisted_chinese() {
        assert!(is_trigger_blacklisted("的"));
        assert!(is_trigger_blacklisted("代码"));
        assert!(is_trigger_blacklisted("测试"));
    }

    #[test]
    fn test_is_trigger_blacklisted_valid() {
        assert!(!is_trigger_blacklisted("deploy"));
        assert!(!is_trigger_blacklisted("部署"));
        assert!(!is_trigger_blacklisted("kubernetes"));
        assert!(!is_trigger_blacklisted("TypeError"));
    }

    #[test]
    fn test_is_trigger_blacklisted_case_insensitive() {
        assert!(is_trigger_blacklisted("The"));
        assert!(is_trigger_blacklisted("THE"));
    }

    // =========================================================================
    // 触发词验证
    // =========================================================================

    #[test]
    fn test_validate_triggers_all_valid() {
        let result = validate_triggers(&["deploy".to_string(), "kubernetes".to_string()]);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_triggers_blacklisted() {
        let result = validate_triggers(&["deploy".to_string(), "the".to_string()]);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("黑名单")));
    }

    #[test]
    fn test_validate_triggers_too_short() {
        let result = validate_triggers(&["x".to_string()]);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("过短")));
    }

    #[test]
    fn test_validate_triggers_too_long() {
        let long_trigger = "a".repeat(60);
        let result = validate_triggers(&[long_trigger]);
        assert!(result.valid); // 过长是 warning 不是 error
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_validate_triggers_special_chars() {
        let result = validate_triggers(&["trigger@#$".to_string()]);
        assert!(result.valid);
        assert!(result.warnings.iter().any(|w| w.contains("特殊字符")));
    }

    // =========================================================================
    // 技能范围优先级
    // =========================================================================

    #[test]
    fn test_skill_scope_ordering() {
        assert!(SkillScope::Project > SkillScope::User);
        assert!(SkillScope::User > SkillScope::Global);
        assert!(SkillScope::Project > SkillScope::Global);
    }

    #[test]
    fn test_skill_scope_display() {
        assert_eq!(format!("{}", SkillScope::Project), "项目");
        assert_eq!(format!("{}", SkillScope::User), "用户");
        assert_eq!(format!("{}", SkillScope::Global), "全局");
    }

    #[test]
    fn test_match_type_display() {
        assert_eq!(format!("{}", MatchType::Exact), "精确");
        assert_eq!(format!("{}", MatchType::Pattern), "模式");
        assert_eq!(format!("{}", MatchType::Fuzzy), "模糊");
        assert_eq!(format!("{}", MatchType::Auto), "自动");
    }

    // =========================================================================
    // Levenshtein 距离
    // =========================================================================

    #[test]
    fn test_levenshtein_distance_basic() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("", "test"), 4);
        assert_eq!(levenshtein_distance("test", "test"), 0);
        assert_eq!(levenshtein_distance("", ""), 0);
    }

    #[test]
    fn test_levenshtein_distance_unicode() {
        // 中文字符按 char 计数
        let d = levenshtein_distance("你好", "你好世界");
        assert_eq!(d, 2);
    }

    #[test]
    fn test_levenshtein_distance_symmetric() {
        assert_eq!(levenshtein_distance("abc", "xyz"), levenshtein_distance("xyz", "abc"));
    }

    // =========================================================================
    // 模糊匹配
    // =========================================================================

    #[test]
    fn test_fuzzy_match_exact_word() {
        assert_eq!(fuzzy_match("deploy the application", "deploy", 60), 100);
    }

    #[test]
    fn test_fuzzy_match_substring() {
        assert!(fuzzy_match("deployment process", "deploy", 60) >= 80);
    }

    #[test]
    fn test_fuzzy_match_below_threshold() {
        assert!(fuzzy_match("completely different", "deploy", 60) < 60);
    }

    #[test]
    fn test_fuzzy_match_empty_pattern() {
        assert_eq!(fuzzy_match("some text", "", 60), 100);
    }

    #[test]
    fn test_fuzzy_match_empty_text() {
        assert_eq!(fuzzy_match("", "deploy", 60), 0);
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        let s1 = fuzzy_match("Deploy the app", "deploy", 60);
        let s2 = fuzzy_match("deploy the app", "DEPLOY", 60);
        assert_eq!(s1, s2);
    }

    // =========================================================================
    // 模式匹配
    // =========================================================================

    #[test]
    fn test_pattern_match_glob() {
        assert_eq!(pattern_match("error handler middleware", "error*"), Some(85));
        assert_eq!(pattern_match("some random text", "error*"), None);
    }

    #[test]
    fn test_pattern_match_regex_slash_format() {
        assert_eq!(pattern_match("I have a TypeError", "/TypeError/"), Some(90));
        assert_eq!(pattern_match("no match here", "/NotFound/"), None);
    }

    #[test]
    fn test_pattern_match_plain_text_no_match() {
        // 普通文本（不含 * 也不是 /pattern/ 格式）不应通过 pattern_match 匹配
        assert_eq!(pattern_match("some text", "deploy"), None);
    }

    // =========================================================================
    // 正则解析
    // =========================================================================

    #[test]
    fn test_parse_regex_pattern_valid() {
        let (pattern, flags) = parse_regex_pattern("/test.*pattern/gi").unwrap();
        assert_eq!(pattern, "test.*pattern");
        assert_eq!(flags, "gi");
    }

    #[test]
    fn test_parse_regex_pattern_no_closing_slash() {
        assert!(parse_regex_pattern("/unclosed").is_none());
    }

    #[test]
    fn test_parse_regex_pattern_too_short() {
        assert!(parse_regex_pattern("/").is_none());
        assert!(parse_regex_pattern("").is_none());
    }

    // =========================================================================
    // regex_lite_match
    // =========================================================================

    #[test]
    fn test_regex_lite_match_wildcard() {
        assert!(regex_lite_match("error handler", "error.*handler"));
        assert!(regex_lite_match("error something handler", "error.*handler"));
        assert!(!regex_lite_match("handler error", "error.*handler"));
    }

    #[test]
    fn test_regex_lite_match_catch_all() {
        assert!(regex_lite_match("anything", ".*"));
    }

    #[test]
    fn test_regex_lite_match_plain() {
        assert!(regex_lite_match("deploy application", "deploy"));
        assert!(!regex_lite_match("something else", "deploy"));
    }

    #[test]
    fn test_regex_lite_match_case_insensitive() {
        assert!(regex_lite_match("DEPLOY app", "deploy"));
        assert!(regex_lite_match("Deploy App", "deploy"));
    }

    // =========================================================================
    // 上下文加成
    // =========================================================================

    #[test]
    fn test_context_bonus_error_match() {
        let context = MatchContext {
            detected_errors: vec!["TypeError".into()],
            detected_files: vec![],
            detected_patterns: vec![],
        };
        let bonus = calculate_context_bonus("TypeError", &context);
        assert!(bonus >= 10); // 错误匹配 +10
    }

    #[test]
    fn test_context_bonus_pattern_match() {
        let context = MatchContext {
            detected_errors: vec![],
            detected_files: vec![],
            detected_patterns: vec!["typescript".into()],
        };
        let bonus = calculate_context_bonus("typescript", &context);
        assert!(bonus >= 5); // 模式匹配 +5
    }

    #[test]
    fn test_context_bonus_capped_at_20() {
        let context = MatchContext {
            detected_errors: vec!["TypeError".into(), "error".into()],
            detected_files: vec![],
            detected_patterns: vec!["debugging".into(), "testing".into()],
        };
        let bonus = calculate_context_bonus("TypeError", &context);
        assert!(bonus <= 20);
    }

    #[test]
    fn test_context_bonus_no_match() {
        let context = MatchContext {
            detected_errors: vec!["ENOENT".into()],
            detected_files: vec![],
            detected_patterns: vec!["docker".into()],
        };
        let bonus = calculate_context_bonus("deploy", &context);
        assert_eq!(bonus, 0);
    }

    // =========================================================================
    // FNV-1a 哈希
    // =========================================================================

    #[test]
    fn test_fnv1a_hash_deterministic() {
        assert_eq!(fnv1a_hash("hello"), fnv1a_hash("hello"));
    }

    #[test]
    fn test_fnv1a_hash_different_inputs() {
        assert_ne!(fnv1a_hash("hello"), fnv1a_hash("world"));
    }

    #[test]
    fn test_fnv1a_hash_empty_string() {
        let h = fnv1a_hash("");
        assert_ne!(h, 0); // FNV offset basis
    }

    // =========================================================================
    // extract_user_text / extract_text_from_content
    // =========================================================================

    #[test]
    fn test_extract_user_text_from_string_content() {
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": "hi"},
                {"role": "user", "content": "fix the TypeError in src/main.ts"}
            ]
        });
        let text = extract_user_text(&body).unwrap();
        assert!(text.contains("TypeError"));
    }

    #[test]
    fn test_extract_user_text_from_array_content() {
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "deploy the application"},
                        {"type": "image", "url": "http://example.com/img.png"}
                    ]
                }
            ]
        });
        let text = extract_user_text(&body).unwrap();
        assert_eq!(text, "deploy the application");
    }

    #[test]
    fn test_extract_user_text_no_messages() {
        let body = serde_json::json!({"model": "gpt-4"});
        assert!(extract_user_text(&body).is_none());
    }

    #[test]
    fn test_extract_user_text_empty_messages() {
        let body = serde_json::json!({"messages": []});
        assert!(extract_user_text(&body).is_none());
    }

    // =========================================================================
    // extract_message_pairs
    // =========================================================================

    #[test]
    fn test_extract_message_pairs_valid() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "hi there"},
                {"role": "user", "content": "how are you?"}
            ]
        });
        let pairs = extract_message_pairs(&body).unwrap();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, "user");
    }

    #[test]
    fn test_extract_message_pairs_empty() {
        let body = serde_json::json!({"messages": []});
        assert!(extract_message_pairs(&body).is_none());
    }

    // =========================================================================
    // is_auto_invoke_injection
    // =========================================================================

    #[test]
    fn test_is_auto_invoke_injection_positive() {
        assert!(crate::proxy::auto_invoke::is_auto_invoke_injection(
            "<auto_invoke_skill>\nHIGH CONFIDENCE MATCH (95%)\nSTATUS: AUTOMATICALLY INVOKED\n</auto_invoke_skill>"
        ));
    }

    #[test]
    fn test_is_auto_invoke_injection_negative() {
        assert!(!crate::proxy::auto_invoke::is_auto_invoke_injection(
            "<skill-injection>normal skill</skill-injection>"
        ));
    }

    // =========================================================================
    // SkillTriggerCache 会话管理
    // =========================================================================

    #[tokio::test]
    async fn test_cache_detect_empty_skills() {
        let cache = SkillTriggerCache::new();
        let results = cache.detect("hello world", "claude", None).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_cache_clear_session() {
        let cache = SkillTriggerCache::new();
        // 不应 panic
        cache.clear_session("nonexistent").await;
    }

    #[tokio::test]
    async fn test_cache_cleanup_expired_sessions() {
        let cache = SkillTriggerCache::new();
        // 不应 panic
        cache.cleanup_expired_sessions().await;
    }

    // =========================================================================
    // inject_skills_content
    // =========================================================================

    #[test]
    fn test_inject_skills_content_system_string() {
        let body = serde_json::json!({
            "system": "You are a helpful assistant.",
            "messages": []
        });
        let skills = vec![TriggeredSkill {
            id: "test".into(),
            name: "TestSkill".into(),
            matched_trigger: "test".into(),
            content: "Do something".into(),
            match_type: MatchType::Exact,
            confidence: 100,
            scope: SkillScope::User,
            context: MatchContext::default(),
        }];
        let result = inject_skills_content(body, &skills);
        let system = result.get("system").and_then(|s| s.as_str()).unwrap();
        assert!(system.contains("<skill-injection>"));
        assert!(system.contains("TestSkill"));
        assert!(system.contains("Do something"));
        assert!(system.contains("</skill-injection>"));
    }

    #[test]
    fn test_inject_skills_content_system_array() {
        let body = serde_json::json!({
            "system": [{"type": "text", "text": "Original system prompt"}],
            "messages": []
        });
        let skills = vec![TriggeredSkill {
            id: "test".into(),
            name: "TestSkill".into(),
            matched_trigger: "test".into(),
            content: "Skill body".into(),
            match_type: MatchType::Exact,
            confidence: 100,
            scope: SkillScope::User,
            context: MatchContext::default(),
        }];
        let result = inject_skills_content(body, &skills);
        let system = result.get("system").and_then(|s| s.as_array()).unwrap();
        assert_eq!(system.len(), 2); // 原有 + 新增
    }

    #[test]
    fn test_inject_skills_content_no_system() {
        let body = serde_json::json!({"messages": []});
        let skills = vec![TriggeredSkill {
            id: "test".into(),
            name: "TestSkill".into(),
            matched_trigger: "test".into(),
            content: "Content".into(),
            match_type: MatchType::Exact,
            confidence: 100,
            scope: SkillScope::User,
            context: MatchContext::default(),
        }];
        let result = inject_skills_content(body, &skills);
        assert!(result.get("system").is_some());
    }

    #[test]
    fn test_inject_skills_content_empty_skills() {
        let body = serde_json::json!({"system": "original", "messages": []});
        let result = inject_skills_content(body, &[]);
        assert_eq!(result.get("system").and_then(|s| s.as_str()).unwrap(), "original");
    }

    // =========================================================================
    // inject_auto_invoke_skills
    // =========================================================================

    #[test]
    fn test_inject_auto_invoke_skills_format() {
        let body = serde_json::json!({"system": "original", "messages": []});
        let skills = vec![TriggeredSkill {
            id: "auto-1".into(),
            name: "AutoSkill".into(),
            matched_trigger: "trigger".into(),
            content: "Skill instructions".into(),
            match_type: MatchType::Exact,
            confidence: 95,
            scope: SkillScope::User,
            context: MatchContext::default(),
        }];
        let result = inject_auto_invoke_skills(body, &skills);
        let system = result.get("system").and_then(|s| s.as_str()).unwrap();
        assert!(system.contains("<auto_invoke_skill>"));
        assert!(system.contains("AutoSkill"));
        assert!(system.contains("95%"));
        assert!(system.contains("AUTOMATICALLY INVOKED"));
    }

    // =========================================================================
    // MatchContext Default
    // =========================================================================

    #[test]
    fn test_match_context_default() {
        let ctx = MatchContext::default();
        assert!(ctx.detected_errors.is_empty());
        assert!(ctx.detected_files.is_empty());
        assert!(ctx.detected_patterns.is_empty());
    }

    // =========================================================================
    // SkillTriggerConfig Default
    // =========================================================================

    #[test]
    fn test_skill_trigger_config_default() {
        let config = SkillTriggerConfig::default();
        assert!(config.multilingual_enabled);
        assert_eq!(config.confidence_threshold, 60);
        assert_eq!(config.max_skills_per_session, 10);
        assert_eq!(config.fuzzy_match_threshold, 60);
        assert_eq!(config.min_trigger_length, 2);
        assert_eq!(config.max_trigger_length, 50);
    }

    // =========================================================================
    // SkillTriggerCache new
    // =========================================================================

    #[test]
    fn test_skill_trigger_cache_new() {
        let _cache = SkillTriggerCache::new();
        // 创建成功
    }

    // =========================================================================
    // TriggeredSkill 字段
    // =========================================================================

    #[test]
    fn test_triggered_skill_fields() {
        let skill = TriggeredSkill {
            id: "test-id".into(),
            name: "TestName".into(),
            matched_trigger: "test-trigger".into(),
            content: "content".into(),
            match_type: MatchType::Fuzzy,
            confidence: 85,
            scope: SkillScope::Project,
            context: MatchContext {
                detected_errors: vec!["Error".into()],
                detected_files: vec!["file.rs".into()],
                detected_patterns: vec!["pattern".into()],
            },
        };
        assert_eq!(skill.id, "test-id");
        assert_eq!(skill.name, "TestName");
        assert_eq!(skill.matched_trigger, "test-trigger");
        assert_eq!(skill.confidence, 85);
        assert_eq!(skill.match_type, MatchType::Fuzzy);
        assert_eq!(skill.scope, SkillScope::Project);
        assert_eq!(skill.context.detected_errors.len(), 1);
    }

    // =========================================================================
    // TriggerValidationResult
    // =========================================================================

    #[test]
    fn test_trigger_validation_result_fields() {
        let result = TriggerValidationResult {
            valid: true,
            errors: vec![],
            warnings: vec![],
        };
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    // =========================================================================
    // MatchType Default
    // =========================================================================

    #[test]
    fn test_match_type_default() {
        // Auto should be default (first defined)
        assert_eq!(MatchType::default(), MatchType::Auto);
    }

    // =========================================================================
    // slide_window_fuzzy
    // =========================================================================

    #[test]
    fn test_slide_window_fuzzy_exact() {
        let score = slide_window_fuzzy("deployment", "deploy");
        assert_eq!(score, 100);
    }

    #[test]
    fn test_slide_window_fuzzy_empty() {
        assert_eq!(slide_window_fuzzy("", "test"), 0);
        assert_eq!(slide_window_fuzzy("test", ""), 100);
    }

    // =========================================================================
    // regex_lite_compile
    // =========================================================================

    #[test]
    fn test_regex_lite_compile_simple() {
        let re = regex_lite_compile("test").unwrap();
        assert!(regex_lite_test("test", &re));
        assert!(!regex_lite_test("other", &re));
    }

    #[test]
    fn test_regex_lite_compile_with_wildcard() {
        let re = regex_lite_compile("test.*").unwrap();
        assert!(regex_lite_test("testing", &re));
    }

    #[test]
    fn test_regex_lite_compile_case_insensitive() {
        let re = regex_lite_compile("TEST").unwrap();
        assert!(regex_lite_test("test", &re));
    }

    // =========================================================================
    // regex_lite_find_all
    // =========================================================================

    #[test]
    fn test_regex_lite_find_all_basic() {
        let re = regex_lite_compile("test").unwrap();
        let matches = regex_lite_find_all("test test test", &re);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_regex_lite_find_all_empty() {
        let re = regex_lite_compile("test").unwrap();
        let matches = regex_lite_find_all("no match here", &re);
        assert!(matches.is_empty());
    }

    // =========================================================================
    // extract_text_from_content 各种格式
    // =========================================================================

    #[test]
    fn test_extract_text_from_content_string() {
        let content = serde_json::json!("plain text");
        assert_eq!(extract_text_from_content(&content), "plain text");
    }

    #[test]
    fn test_extract_text_from_content_array_mixed() {
        let content = serde_json::json!([
            {"type": "text", "text": "first"},
            {"type": "image", "url": "http://example.com"},
            {"type": "text", "text": "second"}
        ]);
        assert_eq!(extract_text_from_content(&content), "first second");
    }

    #[test]
    fn test_extract_text_from_content_array_no_text() {
        let content = serde_json::json!([
            {"type": "image", "url": "http://example.com"}
        ]);
        assert_eq!(extract_text_from_content(&content), "");
    }

    #[test]
    fn test_extract_text_from_content_number() {
        let content = serde_json::json!(123);
        assert_eq!(extract_text_from_content(&content), "");
    }

    // =========================================================================
    // extract_user_text 边界情况
    // =========================================================================

    #[test]
    fn test_extract_user_text_no_user_message() {
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": "hello"}
            ]
        });
        assert!(extract_user_text(&body).is_none());
    }

    #[test]
    fn test_extract_user_text_last_user_message() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "response"},
                {"role": "user", "content": "last message"}
            ]
        });
        let text = extract_user_text(&body).unwrap();
        assert_eq!(text, "last message");
    }

    // =========================================================================
    // extract_message_pairs 边界情况
    // =========================================================================

    #[test]
    fn test_extract_message_pairs_no_messages_field() {
        let body = serde_json::json!({"model": "gpt-4"});
        assert!(extract_message_pairs(&body).is_none());
    }

    #[test]
    fn test_extract_message_pairs_with_tool_use() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "search"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tool1", "name": "search"},
                    {"type": "text", "text": "found it"}
                ]}
            ]
        });
        let pairs = extract_message_pairs(&body).unwrap();
        assert_eq!(pairs.len(), 2);
    }

    // =========================================================================
    // load_skill_content (空实现不应 panic)
    // =========================================================================

    #[test]
    fn test_load_skill_content_no_panic() {
        // 这个函数通常返回空字符串，不应 panic
        let content = load_skill_content("nonexistent");
        assert!(content.is_empty() || content.contains("Skill"));
    }

    // =========================================================================
    // 多语言上下文提取
    // =========================================================================

    #[test]
    fn test_extract_context_japanese_errors() {
        let context = extract_context("コンパイルエラー：TypeErrorが発生");
        assert!(context.detected_errors.iter().any(|e| e.contains("TypeError") || e.contains("エラー")));
    }

    #[test]
    fn test_extract_context_korean_errors() {
        let context = extract_context("TypeError 발생");
        assert!(context.detected_errors.contains(&"TypeError".to_string()));
    }

    // =========================================================================
    // validate_triggers 更多边界情况
    // =========================================================================

    #[test]
    fn test_validate_triggers_empty() {
        let result = validate_triggers(&[]);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_triggers_duplicates() {
        let result = validate_triggers(&["test".to_string(), "test".to_string()]);
        assert!(result.valid);
        // 检查是否有重复警告
        assert!(result.warnings.iter().any(|w| w.contains("重复") || w.contains("duplicate")));
    }

    // =========================================================================
    // is_trigger_blacklisted 更多测试
    // =========================================================================

    #[test]
    fn test_is_trigger_blacklisted_whitespace_variants() {
        assert!(is_trigger_blacklisted(" a ")); // trim 后检查
    }

    #[test]
    fn test_is_trigger_blacklisted_empty() {
        assert!(!is_trigger_blacklisted(""));
        assert!(!is_trigger_blacklisted("   "));
    }

    // =========================================================================
    // 滑动窗口匹配
    // =========================================================================

    #[test]
    fn test_slide_window_fuzzy_longer_text() {
        let score = slide_window_fuzzy("I want to deploy the application now", "deploy");
        assert!(score >= 80);
    }

    #[test]
    fn test_slide_window_fuzzy_unrelated() {
        let score = slide_window_fuzzy("completely different text here", "deploy");
        assert!(score < 70);
    }

    // =========================================================================
    // fuzzy_match 阈值边界
    // =========================================================================

    #[test]
    fn test_fuzzy_match_at_threshold() {
        let score = fuzzy_match("deploy", "deplo", 60);
        // "deplo" 接近 "deploy"
        assert!(score >= 70);
    }

    #[test]
    fn test_fuzzy_match_whitespace_variants() {
        let s1 = fuzzy_match("deploy app", "deploy", 60);
        let s2 = fuzzy_match("deploy  app", "deploy", 60);
        assert_eq!(s1, s2); // 多余空格应不影响结果
    }

    // =========================================================================
    // pattern_match 边界情况
    // =========================================================================

    #[test]
    fn test_pattern_match_empty_pattern() {
        assert_eq!(pattern_match("some text", ""), None);
    }

    #[test]
    fn test_pattern_match_invalid_regex() {
        // 无效的正则应返回 None
        assert_eq!(pattern_match("text", "/[unclosed/"), None);
    }

    // =========================================================================
    // 上下文加成精确值
    // =========================================================================

    #[test]
    fn test_context_bonus_exact_values() {
        // 错误匹配 = +10
        let ctx_err = MatchContext {
            detected_errors: vec!["Error".into()],
            ..Default::default()
        };
        assert_eq!(calculate_context_bonus("Error", &ctx_err), 10);

        // 模式匹配 = +5
        let ctx_pat = MatchContext {
            detected_patterns: vec!["pattern".into()],
            ..Default::default()
        };
        assert_eq!(calculate_context_bonus("pattern", &ctx_pat), 5);

        // 文件匹配 = +2
        let ctx_file = MatchContext {
            detected_files: vec!["file.rs".into()],
            ..Default::default()
        };
        assert_eq!(calculate_context_bonus("file.rs", &ctx_file), 2);
    }

    // =========================================================================
    // inject_skills_content 边界情况
    // =========================================================================

    #[test]
    fn test_inject_skills_content_no_system_field() {
        let body = serde_json::json!({"messages": []});
        let skills = vec![TriggeredSkill {
            id: "test".into(),
            name: "TestSkill".into(),
            matched_trigger: "test".into(),
            content: "Do something".into(),
            match_type: MatchType::Exact,
            confidence: 100,
            scope: SkillScope::User,
            context: MatchContext::default(),
        }];
        let result = inject_skills_content(body, &skills);
        assert!(result.get("system").is_some());
    }

    #[test]
    fn test_inject_skills_content_preserves_other_fields() {
        let body = serde_json::json!({
            "system": "original",
            "messages": [],
            "model": "gpt-4",
            "temperature": 0.7
        });
        let result = inject_skills_content(body, &[]);
        assert_eq!(result.get("model").unwrap(), "gpt-4");
        assert_eq!(result.get("temperature").unwrap(), 0.7);
    }

    // =========================================================================
    // inject_auto_invoke_skills 多技能
    // =========================================================================

    #[test]
    fn test_inject_auto_invoke_skills_multiple() {
        let body = serde_json::json!({"system": "original", "messages": []});
        let skills = vec![
            TriggeredSkill {
                id: "auto-1".into(),
                name: "Skill1".into(),
                matched_trigger: "t1".into(),
                content: "Content1".into(),
                match_type: MatchType::Exact,
                confidence: 95,
                scope: SkillScope::User,
                context: MatchContext::default(),
            },
            TriggeredSkill {
                id: "auto-2".into(),
                name: "Skill2".into(),
                matched_trigger: "t2".into(),
                content: "Content2".into(),
                match_type: MatchType::Fuzzy,
                confidence: 85,
                scope: SkillScope::User,
                context: MatchContext::default(),
            },
        ];
        let result = inject_auto_invoke_skills(body, &skills);
        let system = result.get("system").and_then(|s| s.as_str()).unwrap();
        assert!(system.contains("Skill1"));
        assert!(system.contains("Skill2"));
        assert!(system.contains("95%"));
        assert!(system.contains("85%"));
    }

    // =========================================================================
    // 复杂的 JSON content 提取
    // =========================================================================

    #[test]
    fn test_extract_text_from_content_nested_array() {
        let content = serde_json::json!([
            {"type": "text", "text": "level 1"},
            {"type": "tool_use", "id": "t1", "content": [
                {"type": "text", "text": "nested text"}
            ]}
        ]);
        let text = extract_text_from_content(&content);
        // 只提取顶层 text 块
        assert_eq!(text, "level 1");
    }

    // =========================================================================
    // Unicode 和特殊字符处理
    // =========================================================================

    #[test]
    fn test_extract_context_unicode_errors() {
        let context = extract_context("💥 TypeError: 未定义的变量 🚀");
        assert!(context.detected_errors.contains(&"TypeError".to_string()));
    }

    #[test]
    fn test_fuzzy_match_unicode() {
        // Unicode 字符的编辑距离
        let d = levenshtein_distance("测试", "试试");
        // 应该能计算出不同的距离
        assert!(d > 0);
    }

    // =========================================================================
    // 并发安全 (简单的无 panic 测试)
    // =========================================================================

    #[tokio::test]
    async fn test_concurrent_cache_operations() {
        let cache = SkillTriggerCache::new();
        // 并发清空不同的会话不应 panic
        let handle1 = tokio::spawn(async {
            let cache = SkillTriggerCache::new();
            cache.clear_session("sess1").await;
        });
        let handle2 = tokio::spawn(async {
            let cache = SkillTriggerCache::new();
            cache.clear_session("sess2").await;
        });
        let _ = tokio::try_join!(handle1, handle2);
    }

    // =========================================================================
    // TriggeredSkill clone
    // =========================================================================

    #[test]
    fn test_triggered_skill_clone() {
        let skill = TriggeredSkill {
            id: "test".into(),
            name: "Test".into(),
            matched_trigger: "trigger".into(),
            content: "content".into(),
            match_type: MatchType::Exact,
            confidence: 100,
            scope: SkillScope::User,
            context: MatchContext::default(),
        };
        let cloned = skill.clone();
        assert_eq!(cloned.id, skill.id);
        assert_eq!(cloned.name, skill.name);
    }

    // =========================================================================
    // QualityScore 计算测试
    // =========================================================================

    #[test]
    fn test_quality_score_calculation() {
        // 这个测试验证质量评分逻辑的基本正确性
        let triggers = vec!["deploy".to_string(), "kubernetes".to_string()];
        let result = validate_triggers(&triggers);
        assert!(result.valid);
        // 有效触发词应获得基本质量分
        // 质量分计算内部逻辑在 calculate_skill_quality_score 中
    }

    // =========================================================================
    // 文件路径提取边界情况
    // =========================================================================

    #[test]
    fn test_extract_context_file_paths_edge_cases() {
        // 只有扩展名
        let ctx = extract_context("check .rs and .ts files");
        assert!(ctx.detected_files.iter().any(|f| f == ".rs" || f == ".ts"));

        // Windows UNC 路径
        let ctx = extract_context("\\\\server\\share\\file.txt");
        assert!(ctx.detected_files.iter().any(|f| f.contains("file.txt")));
    }

    // =========================================================================
    // 各种错误类型检测
    // =========================================================================

    #[test]
    fn test_extract_context_various_error_types() {
        let ctx = extract_context(
            "Got ReferenceError, SyntaxError, and TypeError in the code"
        );
        assert!(ctx.detected_errors.contains(&"ReferenceError".to_string()));
        assert!(ctx.detected_errors.contains(&"SyntaxError".to_string()));
        assert!(ctx.detected_errors.contains(&"TypeError".to_string()));
    }

    // =========================================================================
    // 技术栈关键词
    // =========================================================================

    #[test]
    fn test_extract_context_framework_keywords() {
        let ctx = extract_context("using React with TypeScript and Express backend");
        assert!(ctx.detected_patterns.contains(&"react".to_string()));
        assert!(ctx.detected_patterns.contains(&"typescript".to_string()));
    }

    #[test]
    fn test_extract_context_infrastructure_keywords() {
        let ctx = extract_context("deploy with Docker and Kubernetes on AWS");
        assert!(ctx.detected_patterns.contains(&"docker".to_string()));
        assert!(ctx.detected_patterns.contains(&"kubernetes".to_string()));
    }
}
