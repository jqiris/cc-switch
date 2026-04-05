//! Skill 压缩模块
//!
//! 为本地模型等上下文窗口有限的 Provider 提供 skill 注入压缩能力：
//! - Catalog 模式：只注入 skill 的名称+描述+触发词（~100字符/skill）
//! - 结构性压缩：移除代码块、HTML注释、多余空行
//! - 预算控制：限制 skill 注入的总字符数

use std::collections::HashMap;

// =============================================================================
// 注入模式
// =============================================================================

/// Skill 注入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillInjectionMode {
    /// 完整注入所有触发 skill 的内容（云端模型默认）
    #[default]
    Full,
    /// Catalog 模式：只注入 name+triggers 一行摘要（本地模型默认）
    Catalog,
    /// 完全禁用 skill 注入
    #[allow(dead_code)]
    None,
}

impl std::fmt::Display for SkillInjectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Catalog => write!(f, "catalog"),
            Self::None => write!(f, "none"),
        }
    }
}

// =============================================================================
// 预算配置
// =============================================================================

/// Skill 注入预算配置
///
/// 控制每次请求中 skill 注入的总字符数上限，
/// 以及高置信度 skill 完整注入的最大数量。
#[derive(Debug, Clone)]
pub struct SkillBudgetConfig {
    /// 预算控制总开关
    pub enabled: bool,
    /// 所有 skill 注入的总字符预算上限（默认 32000）
    pub injection_char_budget: usize,
    /// 单个 skill 压缩后的最大字符数（默认 4000）
    pub single_skill_char_budget: usize,
    /// 高置信度完整注入的最大 skill 数量（默认 3）
    pub max_full_skills: usize,
    /// 是否启用结构性压缩（默认 true）
    pub structural_compression: bool,
    /// 压缩后单 skill 最大行数（默认 150）
    pub max_compressed_lines: usize,
    /// Provider 级策略覆盖（provider_id -> mode）
    pub provider_overrides: HashMap<String, SkillInjectionMode>,
}

impl Default for SkillBudgetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            injection_char_budget: 32_000,
            single_skill_char_budget: 4_000,
            max_full_skills: 3,
            structural_compression: true,
            max_compressed_lines: 150,
            provider_overrides: HashMap::new(),
        }
    }
}

// =============================================================================
// Provider 级策略解析
// =============================================================================

/// 根据 Provider 配置解析 skill 注入模式
///
/// 优先级：provider_overrides > meta.skill_policy > apiFormat 自动判断 > Full
pub fn resolve_injection_mode(
    provider_id: &str,
    _provider_meta_api_format: Option<&str>,
    _adapter_name: &str,
    is_openai_compatible: bool,
    budget_config: &SkillBudgetConfig,
) -> SkillInjectionMode {
    // 1. 检查 provider 级覆盖
    if let Some(mode) = budget_config.provider_overrides.get(provider_id) {
        log::debug!(
            "[SkillCompressor] Provider '{}' 使用覆盖策略: {}",
            provider_id,
            mode
        );
        return *mode;
    }

    if !budget_config.enabled {
        return SkillInjectionMode::Full;
    }

    // 2. 根据 apiFormat 自动判断
    if is_openai_compatible {
        log::debug!(
            "[SkillCompressor] Provider '{}' 检测到 OpenAI 兼容格式，使用 Catalog 模式",
            provider_id
        );
        return SkillInjectionMode::Catalog;
    }

    // 3. 默认 Full 模式（云端模型）
    SkillInjectionMode::Full
}

// =============================================================================
// 结构性压缩
// =============================================================================

/// 结构性压缩 skill 内容
///
/// 移除代码块、HTML注释、多余空行等，保留核心指令。
/// 返回 (压缩后内容, 压缩率 0.0-1.0)
pub fn compress_skill_content(content: &str, max_lines: usize) -> (String, f32) {
    let original_len = content.len();
    if original_len == 0 {
        return (String::new(), 0.0);
    }

    let mut result_lines: Vec<String> = Vec::new();
    let mut in_code_block = false;
    let mut consecutive_blank = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // 跳过 YAML frontmatter 分隔符
        if trimmed == "---" {
            continue;
        }

        // 跳过 HTML 注释
        if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
            continue;
        }

        // 跟踪代码块状态
        if trimmed.starts_with("```") {
            if in_code_block {
                // 代码块结束
                in_code_block = false;
                consecutive_blank = 0;
                continue;
            } else {
                // 代码块开始 — 跳过整个代码块
                in_code_block = true;
                continue;
            }
        }

        if in_code_block {
            continue;
        }

        // 处理空行
        if trimmed.is_empty() {
            consecutive_blank += 1;
            if consecutive_blank <= 1 {
                result_lines.push(String::new());
            }
            continue;
        } else {
            consecutive_blank = 0;
        }

        result_lines.push(line.to_string());
    }

    // 截断到最大行数
    if result_lines.len() > max_lines {
        result_lines.truncate(max_lines);
        result_lines.push("... (内容已截断)".to_string());
    }

    let compressed = result_lines.join("\n");
    let compressed_len = compressed.len();

    let ratio = if original_len > 0 {
        1.0 - (compressed_len as f32 / original_len as f32)
    } else {
        0.0
    };

    (compressed, ratio)
}

// =============================================================================
// Catalog 格式化
// =============================================================================

/// 生成 catalog 条目（一行摘要）
///
/// 格式：`- [name] description | 触发词: trigger1, trigger2 | 置信度: N%`
pub fn format_catalog_entry(
    name: &str,
    matched_trigger: &str,
    confidence: usize,
) -> String {
    format!(
        "- [{}] 触发词=\"{}\" | 置信度: {}%",
        name, matched_trigger, confidence
    )
}

/// 将多个 skill 格式化为 catalog 注入内容
///
/// 整体用 <skill-catalog> 标签包裹，每个 skill 一行摘要。
pub fn format_catalog_block(
    entries: &[(String, String, usize)], // (name, matched_trigger, confidence)
) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(entries.len() + 3);
    parts.push("<skill-catalog>".to_string());
    parts.push("已触发技能（已压缩为目录模式以适配上下文窗口）：".to_string());
    parts.push(String::new());

    for (name, trigger, confidence) in entries {
        parts.push(format_catalog_entry(name, trigger, *confidence));
    }

    parts.push(String::new());
    parts.push("</skill-catalog>".to_string());

    parts.join("\n")
}

// =============================================================================
// Token 估算
// =============================================================================

/// 估算文本的 token 数（保守估计）
///
/// 规则：
/// - 中文字符：1 字符 ≈ 1.5 tokens
/// - 英文单词：1 单词 ≈ 1.3 tokens
/// - 混合文本取加权平均
pub fn estimate_tokens(text: &str) -> usize {
    let mut chinese_chars = 0usize;
    let mut english_words = 0usize;
    let mut other_chars = 0usize;

    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_ascii() {
            if ch.is_alphabetic() {
                if !in_word {
                    english_words += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
                if !ch.is_whitespace() {
                    other_chars += 1;
                }
            }
        } else if is_cjk_char(ch) {
            chinese_chars += 1;
            in_word = false;
        } else {
            other_chars += 1;
            in_word = false;
        }
    }

    // 保守估算
    let chinese_tokens = (chinese_chars as f64 * 1.5) as usize;
    let english_tokens = ((english_words as f64) * 1.3) as usize;
    let other_tokens = other_chars / 3; // 符号/数字粗略

    chinese_tokens + english_tokens + other_tokens
}

/// 判断字符是否为 CJK（中日韩）
fn is_cjk_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}' |   // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}' |   // CJK Unified Ideographs Extension A
        '\u{2E80}'..='\u{2EFF}' |   // CJK Radicals Supplement
        '\u{3000}'..='\u{303F}' |   // CJK Symbols and Punctuation
        '\u{FF00}'..='\u{FFEF}'     // Halfwidth and Fullwidth Forms
    )
}

// =============================================================================
// 预算感知的 skill 分配
// =============================================================================

/// 预算分配结果
pub struct BudgetAllocation {
    /// 完整注入的 skill 列表（含压缩后的内容）
    pub full_skills: Vec<(String, String, usize)>, // (name, content, confidence)
    /// 降级为 catalog 的 skill 列表
    pub catalog_skills: Vec<(String, String, usize)>, // (name, trigger, confidence)
    /// 总注入字符数
    pub total_chars: usize,
    /// 估算 token 数
    pub estimated_tokens: usize,
}

/// 根据预算分配 skill 注入策略
///
/// 将触发的 skill 分为"完整注入"和"catalog 降级"两组：
/// 1. 前 max_full_skills 个高置信度 skill 完整注入（可选压缩）
/// 2. 超出预算或数量的 skill 降级为 catalog 条目
pub fn allocate_budget(
    triggered_skills: &[(String, String, String, usize)], // (name, content, trigger, confidence)
    budget_config: &SkillBudgetConfig,
) -> BudgetAllocation {
    let mut full_skills = Vec::new();
    let mut catalog_skills = Vec::new();
    let mut remaining_budget = budget_config.injection_char_budget;
    let mut full_count = 0;

    for (name, content, trigger, confidence) in triggered_skills {
        // 决定是否完整注入
        let should_full = full_count < budget_config.max_full_skills
            && remaining_budget > budget_config.single_skill_char_budget;

        if should_full {
            // 可选：结构性压缩
            let (injected_content, _compression_ratio) =
                if budget_config.structural_compression && content.len() > budget_config.single_skill_char_budget {
                    let (compressed, ratio) = compress_skill_content(
                        content,
                        budget_config.max_compressed_lines,
                    );
                    if ratio > 0.1 {
                        // 压缩节省 >10% 才使用压缩版
                        log::debug!(
                            "[SkillCompressor] 压缩 skill '{}': {:.0}% → {} 字符",
                            name,
                            ratio * 100.0,
                            compressed.len()
                        );
                        (compressed, ratio)
                    } else {
                        (content.clone(), 0.0)
                    }
                } else {
                    (content.clone(), 0.0)
                };

            let content_len = injected_content.len();
            if content_len <= remaining_budget {
                remaining_budget -= content_len;
                full_skills.push((name.clone(), injected_content, *confidence));
                full_count += 1;
            } else {
                // 预算不足，降级为 catalog
                catalog_skills.push((name.clone(), trigger.clone(), *confidence));
            }
        } else {
            // 超过 max_full_skills 或预算不足，降级为 catalog
            catalog_skills.push((name.clone(), trigger.clone(), *confidence));
        }
    }

    // 计算总量
    let full_total: usize = full_skills.iter().map(|(_, c, _)| c.len()).sum();
    let catalog_block = format_catalog_block(&catalog_skills);
    let total_chars = full_total + catalog_block.len();
    let estimated_tokens = estimate_tokens(&format!(
        "{}{}",
        full_skills.iter().map(|(_, c, _)| c.as_str()).collect::<Vec<_>>().join(""),
        &catalog_block
    ));

    BudgetAllocation {
        full_skills,
        catalog_skills,
        total_chars,
        estimated_tokens,
    }
}

// =============================================================================
// 注入到请求体
// =============================================================================

/// 将预算分配结果注入到请求体的 system 字段
///
/// 完整 skill 使用现有的 <skill-injection> 格式，
/// catalog 使用 <skill-catalog> 格式追加在末尾。
pub fn inject_with_budget(
    mut body: serde_json::Value,
    allocation: &BudgetAllocation,
    scope: &str,
) -> serde_json::Value {
    let mut injection_parts = Vec::new();

    // 完整注入的 skills
    if !allocation.full_skills.is_empty() {
        injection_parts.push("<skill-injection>".to_string());
        injection_parts.push(String::new());
        injection_parts.push("## 已触发的技能（完整内容）".to_string());
        injection_parts.push(String::new());

        for (name, content, confidence) in &allocation.full_skills {
            injection_parts.push(format!("### {}", name));
            injection_parts.push(format!("**来源:** {}级技能", scope));
            injection_parts.push(format!("**置信度:** {}%", confidence));
            injection_parts.push(String::new());
            injection_parts.push(content.clone());
            injection_parts.push(String::new());
            injection_parts.push("---".to_string());
            injection_parts.push(String::new());
        }

        injection_parts.push("</skill-injection>".to_string());
    }

    // Catalog 降级的 skills
    if !allocation.catalog_skills.is_empty() {
        let catalog_block = format_catalog_block(&allocation.catalog_skills);
        if !catalog_block.is_empty() {
            if !injection_parts.is_empty() {
                injection_parts.push(String::new());
            }
            injection_parts.push(catalog_block);
        }
    }

    if injection_parts.is_empty() {
        return body;
    }

    let injection = injection_parts.join("\n");

    log::info!(
        "[SkillCompressor] 注入完成: {} 个完整, {} 个 catalog, 总计 {} 字符 (约 {} tokens)",
        allocation.full_skills.len(),
        allocation.catalog_skills.len(),
        allocation.total_chars,
        allocation.estimated_tokens,
    );

    // 注入到 system 字段
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

// =============================================================================
// 三级工具压缩（Tool Definitions 压缩）— 旧方案，已被无损方案取代
// =============================================================================

/// 三级工具压缩结果
#[derive(Debug, Clone)]
pub struct TieredToolCompressionResult {
    /// Tier 1 + Tier 2 的工具定义（保留为 OpenAI tools 数组）
    pub compressed_tools: Vec<serde_json::Value>,
    /// Tier 3 被压缩为 catalog 的工具 (name, description)
    pub catalog_tools: Vec<(String, String)>,
    /// 压缩统计
    pub stats: TieredToolCompressionStats,
}

/// 三级压缩统计信息
#[derive(Debug, Clone)]
pub struct TieredToolCompressionStats {
    pub original_count: usize,
    pub tier1_count: usize,
    pub tier2_count: usize,
    pub tier3_count: usize,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
}

/// 三级工具压缩
///
/// 借鉴 rtk 的分级策略 + 结构感知截断 + Tee 恢复机制
///
/// # 压缩策略
///
/// **Tier 1 — 核心工具（完整保留）**：
/// - Claude Code 内置工具（无 `mcp__` 前缀，无 `-` 连字符）
/// - 保留完整 JSON schema
///
/// **Tier 2 — 常用工具（压缩 schema）**：
/// - Agent, EnterWorktree, ExitWorktree, MCP 相关工具
/// - 保留 `name`、`type`，截断 `description` 到 150 字符
/// - `parameters` 中保留 `type`、`required`、`properties` 的 key 和 `type`，移除所有 `description`
///
/// **Tier 3 — MCP 工具（name + 短描述）**：
/// - 所有 `mcp__` 前缀的工具（Tier 1 中已排除的）
/// - 只保留 `name` + `description` 截断到 80 字符
/// - 返回在 catalog_tools 中，由调用方决定如何注入
///
/// # 参数
///
/// * `tools` - OpenAI 格式的 tool definitions 数组
///
/// # 返回
///
/// 返回压缩结果，包含：
/// - compressed_tools: 可直接用于 OpenAI tools 参数的工具定义数组
/// - catalog_tools: Tier 3 工具的 (name, description) 列表
/// - stats: 压缩统计信息
///
/// # 示例
///
/// ```rust
/// let tools = vec![/* ... */];
/// let result = tiered_compress_tool_definitions(&tools);
/// println!("压缩: {} -> {} 工具", result.stats.original_count, result.compressed_tools.len());
/// println!("Tier 3: {} 工具转为 catalog", result.stats.tier3_count);
/// ```
pub fn tiered_compress_tool_definitions(
    tools: &[serde_json::Value],
) -> TieredToolCompressionResult {
    let original_count = tools.len();
    let original_bytes = serde_json::to_vec(tools)
        .map(|v| v.len())
        .unwrap_or(0);

    let mut tier1_tools = Vec::new();
    let mut tier2_tools = Vec::new();
    let mut tier3_tools = Vec::new();

    for tool in tools {
        // 提取工具名称
        let tool_name = tool
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        // 分类处理
        if is_tiered_core_tool(tool_name) {
            // Tier 1: 完整保留
            tier1_tools.push(tool.clone());
        } else if is_common_tool(tool_name) {
            // Tier 2: 压缩 schema
            let mut compressed = tool.clone();
            compress_tool_schema(&mut compressed);
            tier2_tools.push(compressed);
        } else {
            // Tier 3: 提取 name + description
            if let Some(description) = tool
                .get("function")
                .and_then(|f| f.get("description"))
                .and_then(|d| d.as_str())
            {
                let truncated = truncate_description(description, 80);
                tier3_tools.push((tool_name.to_string(), truncated));
            }
        }
    }

    // 合并 Tier 1 和 Tier 2
    let tier1_count = tier1_tools.len();
    let tier2_count = tier2_tools.len();
    let mut compressed_tools = tier1_tools;
    compressed_tools.extend(tier2_tools);

    // 计算压缩后大小
    let compressed_bytes = serde_json::to_vec(&compressed_tools)
        .map(|v| v.len())
        .unwrap_or(0);

    let stats = TieredToolCompressionStats {
        original_count,
        tier1_count,
        tier2_count,
        tier3_count: tier3_tools.len(),
        original_bytes,
        compressed_bytes,
    };

    log::info!(
        "[ToolCompressor] 压缩完成: {} -> {} 工具 (Tier1: {}, Tier2: {}, Tier3: {}), {} -> {} 字节",
        stats.original_count,
        compressed_tools.len(),
        stats.tier1_count,
        stats.tier2_count,
        stats.tier3_count,
        stats.original_bytes,
        stats.compressed_bytes
    );

    TieredToolCompressionResult {
        compressed_tools,
        catalog_tools: tier3_tools,
        stats,
    }
}

/// 判断工具是否为 Tier 1 核心工具（三级方案）
///
/// Tier 1 核心工具白名单：
/// - Claude Code 内置工具（无 `mcp__` 前缀，无 `-` 连字符）
fn is_tiered_core_tool(tool_name: &str) -> bool {
    // 无 mcp__ 前缀且无 - 连字符的工具为核心工具
    !tool_name.starts_with("mcp__") && !tool_name.contains('-')
}

/// 判断工具是否为 Tier 2 常用工具
///
/// Tier 2 包括：
/// - Agent 相关工具（含 - 连字符但不含 mcp__ 前缀）
/// - MCP 管理工具（如 ListMcpResources, ReadMcpResource）
fn is_common_tool(tool_name: &str) -> bool {
    // 非 mcp__ 但含 - 的工具（如 Agent, EnterWorktree）
    if !tool_name.starts_with("mcp__") && tool_name.contains('-') {
        return true;
    }

    // MCP 相关管理工具
    matches!(
        tool_name,
        "ListMcpResources" | "ReadMcpResource" | "mcp__sequential-thinking__sequentialthinking"
    )
}

/// 压缩工具 schema（Tier 2）
///
/// 压缩策略：
/// - 保留 name、type（"function"）
/// - 截断 description 到 150 字符
/// - parameters 中：
///   - 保留 type、required、properties 的 key 和 type
///   - 移除所有 description 字段
///   - 移除 enum、default、$schema 等非必要字段
///   - 移除 additionalProperties
fn compress_tool_schema(tool: &mut serde_json::Value) {
    if let Some(function) = tool.get_mut("function") {
        // 截断 description
        if let Some(desc) = function.get_mut("description") {
            if let Some(desc_str) = desc.as_str() {
                *desc = serde_json::json!(truncate_description(desc_str, 150));
            }
        }

        // 压缩 parameters
        if let Some(params) = function.get_mut("parameters") {
            if let Some(obj) = params.as_object_mut() {
                // 移除非必要字段
                obj.remove("additionalProperties");
                obj.remove("$schema");

                // 压缩 properties
                if let Some(props) = obj.get_mut("properties") {
                    if let Some(props_obj) = props.as_object_mut() {
                        for (_prop_name, prop_value) in props_obj.iter_mut() {
                            if let Some(prop_obj) = prop_value.as_object_mut() {
                                // 只保留 type，移除其他字段
                                let prop_type = prop_obj.get("type").cloned();
                                *prop_obj = serde_json::Map::new();

                                if let Some(t) = prop_type {
                                    prop_obj.insert("type".to_string(), t);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 截断 description 到指定字符数
///
/// 在词边界处截断，避免在 UTF-8 字符中间截断
fn truncate_description(desc: &str, max_chars: usize) -> String {
    if desc.len() <= max_chars {
        return desc.to_string();
    }

    // 找到截断点
    let truncate_at = if let Some(pos) = desc[..max_chars].rfind(char::is_whitespace) {
        pos
    } else {
        max_chars
    };

    let mut truncated = desc[..truncate_at].to_string();
    truncated.push_str("...");

    truncated
}

/// 生成 Tier 3 的 catalog 块
///
/// 格式化为 XML 块，可在 system prompt 中追加
///
/// # 示例
///
/// ```rust
/// let catalog = vec![
///     ("mcp__chrome-devtools__click".to_string(), "Clicks on element...".to_string()),
/// ];
/// let block = format_tool_catalog(&catalog);
/// ```
pub fn format_tool_catalog(tools: &[(String, String)]) -> String {
    if tools.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(tools.len() + 3);
    parts.push("<available-mcp-tools>".to_string());
    parts.push("以下 MCP 工具可用（已压缩为目录模式）：".to_string());
    parts.push(String::new());

    for (name, description) in tools {
        parts.push(format!("- {}: {}", name, description));
    }

    parts.push(String::new());
    parts.push("</available-mcp-tools>".to_string());

    parts.join("\n")
}

// =============================================================================
// 工具压缩（无损方案：摘要 + 全量缓存）
// =============================================================================
//
// 设计思路（借鉴 rtk 的 Tee 恢复机制）：
// - 核心工具保留完整 schema，AI 可直接调用
// - 非核心工具降级为 catalog 索引（name + 短描述）
// - 完整定义缓存到本地文件，AI 按需 Read 获取
// - 压缩不是损失，信息始终可恢复

/// 工具压缩统计
#[derive(Debug, Clone)]
pub struct ToolCompressionStats {
    /// 原始工具数量
    pub original_count: usize,
    /// 核心工具数量（完整保留）
    pub core_count: usize,
    /// 索引工具数量（降级为 catalog）
    pub catalog_count: usize,
    /// 原始工具定义总字节数
    pub original_bytes: usize,
    /// 压缩后工具定义字节数（仅核心工具）
    pub compressed_bytes: usize,
}

/// 工具压缩结果
pub struct ToolCompressionResult {
    /// 核心工具（完整 schema，保留为 OpenAI tools 数组）
    pub compressed_tools: Vec<serde_json::Value>,
    /// 非核心工具按 provider 分组: provider_name -> [(tool_name, description)]
    pub catalog_by_provider: HashMap<String, Vec<(String, String)>>,
    /// 压缩统计
    pub stats: ToolCompressionStats,
}

/// 核心工具名称白名单
/// 这些工具是 Claude Code 的基础能力，必须保留完整 schema
const CORE_TOOLS: &[&str] = &[
    "Read", "Write", "Edit", "Glob", "Grep", "Bash",
    "TodoWrite", "WebSearch", "AskUserQuestion",
    "EnterPlanMode", "ExitPlanMode", "NotebookEdit",
    "ReadMcpResource", "ListMcpResources", "Skill",
    "CronCreate", "CronDelete", "CronList",
    "TaskOutput", "TaskStop", "WebFetch",
    // sequential-thinking 是推理增强工具，保留完整
    "mcp__sequential-thinking__sequentialthinking",
];

/// 判断是否为核心工具
pub fn is_core_tool(tool_name: &str) -> bool {
    CORE_TOOLS.contains(&tool_name)
}

/// 从工具名提取 provider（如 "mcp__chrome-devtools__click" → "chrome-devtools"）
pub fn extract_provider(tool_name: &str) -> Option<&str> {
    let name = tool_name.strip_prefix("mcp__")?;
    name.split_once("__").map(|(provider, _)| provider)
}

/// 在词边界处截断文本（UTF-8 安全）
pub fn truncate_at_word_boundary(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    // 在 max_len 范围内找最后一个空白字符
    let end = &s[..max_len];
    if let Some(pos) = end.rfind(|c: char| c.is_whitespace()) {
        format!("{}...", &s[..pos])
    } else {
        format!("{}...", end)
    }
}

/// 从 OpenAI 工具定义中提取名称
fn extract_tool_name(tool: &serde_json::Value) -> Option<&str> {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
}

/// 从 OpenAI 工具定义中提取描述
fn extract_tool_description(tool: &serde_json::Value) -> Option<&str> {
    tool.get("function")
        .and_then(|f| f.get("description"))
        .and_then(|d| d.as_str())
}

/// 无损工具压缩
///
/// 核心工具保留完整 schema，其余工具降级为 catalog 索引，
/// 完整定义通过 save_tool_cache() 保存到本地文件供 AI 按需读取。
pub fn compress_tool_definitions(
    tools: &[serde_json::Value],
) -> ToolCompressionResult {
    let original_count = tools.len();
    let original_bytes: usize = tools
        .iter()
        .map(|t| serde_json::to_string(t).unwrap_or_default().len())
        .sum();

    let mut compressed_tools = Vec::new();
    let mut catalog_by_provider: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut core_count = 0;

    for tool in tools {
        let tool_name = extract_tool_name(tool).unwrap_or("");

        if is_core_tool(tool_name) {
            compressed_tools.push(tool.clone());
            core_count += 1;
        } else {
            let desc = extract_tool_description(tool).unwrap_or("");
            let short_desc = truncate_at_word_boundary(desc, 100);

            let provider = extract_provider(tool_name)
                .unwrap_or("other")
                .to_string();

            catalog_by_provider
                .entry(provider)
                .or_default()
                .push((tool_name.to_string(), short_desc));
        }
    }

    let compressed_bytes: usize = compressed_tools
        .iter()
        .map(|t| serde_json::to_string(t).unwrap_or_default().len())
        .sum();

    ToolCompressionResult {
        compressed_tools,
        catalog_by_provider,
        stats: ToolCompressionStats {
            original_count,
            core_count,
            catalog_count: original_count - core_count,
            original_bytes,
            compressed_bytes,
        },
    }
}

/// 生成工具 catalog 注入块
///
/// 告知 AI 有哪些可用工具，以及如何获取完整参数定义。
/// 借鉴 rtk 的 Tee 恢复机制：压缩不是损失，信息始终可恢复。
pub fn format_tool_catalog_block(
    catalog_by_provider: &HashMap<String, Vec<(String, String)>>,
    cache_dir: &str,
) -> String {
    if catalog_by_provider.is_empty() {
        return String::new();
    }

    let total_tools: usize = catalog_by_provider.values().map(|v| v.len()).sum();

    let mut parts = Vec::with_capacity(total_tools + catalog_by_provider.len() + 8);
    parts.push("<available-mcp-tools>".to_string());
    parts.push(format!(
        "以下 {} 个工具的完整参数定义已按 provider 缓存到本地。如需调用某个工具，请先用 Read 工具读取对应 provider 文件查看完整参数：",
        total_tools
    ));
    parts.push(String::new());

    // Provider 文件索引
    parts.push("Provider 缓存文件：".to_string());
    let mut providers: Vec<_> = catalog_by_provider.iter().collect();
    providers.sort_by_key(|(name, _)| (*name).clone());
    for (provider, tools) in &providers {
        let file_name = format!("{}/{}.json", cache_dir, provider);
        parts.push(format!("- {} ({} tools): {}", provider, tools.len(), file_name));
    }
    parts.push(String::new());

    // 工具列表
    parts.push("工具列表：".to_string());
    for (_provider, tools) in &providers {
        for (name, desc) in tools.iter() {
            parts.push(format!("- {}: {}", name, desc));
        }
    }

    parts.push(String::new());
    parts.push("</available-mcp-tools>".to_string());

    parts.join("\n")
}

/// 将非核心工具的完整定义按 provider 分组保存到缓存目录
///
/// 每个 provider 一个 JSON 文件，便于 AI 按需读取。
pub fn save_tool_cache(
    tools: &[serde_json::Value],
    cache_dir: &str,
) -> std::io::Result<()> {
    // 创建缓存目录
    std::fs::create_dir_all(cache_dir)?;

    // 按 provider 分组
    let mut by_provider: HashMap<String, Vec<&serde_json::Value>> = HashMap::new();
    for tool in tools {
        let tool_name = extract_tool_name(tool).unwrap_or("");
        if is_core_tool(tool_name) {
            continue; // 核心工具不需要缓存（已在请求体中完整保留）
        }
        let provider = extract_provider(tool_name)
            .unwrap_or("other")
            .to_string();
        by_provider
            .entry(provider)
            .or_default()
            .push(tool);
    }

    // 每个 provider 写一个文件
    for (provider, provider_tools) in &by_provider {
        let file_path = format!("{}/{}.json", cache_dir, provider);
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "provider": provider,
            "tool_count": provider_tools.len(),
            "tools": provider_tools,
        }))?;
        std::fs::write(&file_path, json)?;
        log::debug!(
            "[ToolCompressor] 缓存 {} 个工具到 {}",
            provider_tools.len(),
            file_path
        );
    }

    Ok(())
}

/// 获取工具缓存目录路径
pub fn get_tool_cache_dir() -> String {
    std::env::temp_dir()
        .join("cc-switch-tool-cache")
        .to_string_lossy()
        .to_string()
}
