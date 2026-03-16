//! 技能触发检测模块
//!
//! 从数据库加载已安装的技能，检测用户消息中的触发关键词，自动注入技能内容
//! 参考 oh-my-claudecode 的 skill-injector 实现

use crate::database::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 检测到的技能
#[derive(Debug, Clone)]
pub struct TriggeredSkill {
    /// 技能 ID
    pub id: String,
    /// 技能名称
    pub name: String,
    /// 匹配的触发词
    pub matched_trigger: String,
    /// 技能内容（SKILL.md 的完整内容）
    pub content: String,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
}

/// 技能触发缓存
pub struct SkillTriggerCache {
    /// 技能列表（包含触发词）
    skills: RwLock<Vec<SkillWithTriggers>>,
}

/// 带触发词的技能
#[derive(Debug, Clone)]
struct SkillWithTriggers {
    id: String,
    name: String,
    directory: String,
    triggers: Vec<String>,
    enabled_apps: SkillApps,
}

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

impl SkillTriggerCache {
    /// 创建新的技能触发缓存
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(Vec::new()),
        }
    }

    /// 从数据库加载技能
    pub async fn load_from_db(&self, db: &Arc<Database>) {
        let installed_skills = match db.get_all_installed_skills() {
            Ok(skills) => skills,
            Err(e) => {
                log::error!("[SkillTrigger] 加载技能失败: {}", e);
                return;
            }
        };

        let mut skills_with_triggers = Vec::new();

        for (_, skill) in installed_skills {
            // 读取 SKILL.md 解析 triggers
            let triggers = load_skill_triggers(&skill.directory);

            if !triggers.is_empty() {
                log::debug!(
                    "[SkillTrigger] 技能 '{}' 有 {} 个触发词: {:?}",
                    skill.name,
                    triggers.len(),
                    triggers
                );

                skills_with_triggers.push(SkillWithTriggers {
                    id: skill.id,
                    name: skill.name,
                    directory: skill.directory,
                    triggers,
                    enabled_apps: SkillApps::from(skill.apps),
                });
            }
        }

        let mut skills = self.skills.write().await;
        *skills = skills_with_triggers;

        log::info!("[SkillTrigger] 已加载 {} 个带触发词的技能", skills.len());
    }

    /// 检测用户消息中的技能触发
    pub async fn detect(&self, user_text: &str, app_type: &str) -> Option<TriggeredSkill> {
        let skills = self.skills.read().await;
        let text_lower = user_text.to_lowercase();

        for skill in skills.iter() {
            // 检查该应用是否启用了此技能
            let is_enabled = match app_type {
                "claude" => skill.enabled_apps.claude,
                "codex" => skill.enabled_apps.codex,
                "gemini" => skill.enabled_apps.gemini,
                "opencode" => skill.enabled_apps.opencode,
                _ => false,
            };

            if !is_enabled {
                continue;
            }

            // 检查每个触发词
            for trigger in &skill.triggers {
                let trigger_lower = trigger.to_lowercase();

                // 精确匹配
                if text_lower.contains(&trigger_lower) {
                    // 加载技能内容
                    let content = load_skill_content(&skill.directory);

                    return Some(TriggeredSkill {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                        matched_trigger: trigger.clone(),
                        content,
                        confidence: 1.0,
                    });
                }
            }
        }

        None
    }
}

impl Default for SkillTriggerCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 从 SKILL.md 加载触发词
fn load_skill_triggers(directory: &str) -> Vec<String> {
    let skill_path = get_ssot_skill_path(directory);
    let skill_md = skill_path.join("SKILL.md");

    if !skill_md.exists() {
        return Vec::new();
    }

    match crate::services::skill::SkillService::parse_skill_metadata_static(&skill_md) {
        Ok(meta) => meta.triggers,
        Err(e) => {
            log::warn!("[SkillTrigger] 解析技能元数据失败: {}", e);
            Vec::new()
        }
    }
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
    let ssot_dir = crate::config::get_app_config_dir()
        .join("skills")
        .join(directory);

    ssot_dir
}

/// 全局技能触发缓存
static SKILL_TRIGGER_CACHE: once_cell::sync::Lazy<SkillTriggerCache> =
    once_cell::sync::Lazy::new(SkillTriggerCache::new);

/// 获取全局技能触发缓存
pub fn get_skill_trigger_cache() -> &'static SkillTriggerCache {
    &SKILL_TRIGGER_CACHE
}

/// 从用户消息中提取文本
fn extract_user_text(body: &serde_json::Value) -> Option<String> {
    let messages = body.get("messages").and_then(|m| m.as_array())?;

    // 获取最后一条用户消息
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
///
/// 返回 (修改后的请求体, 检测到的技能)
pub async fn detect_and_inject_skill(
    body: serde_json::Value,
    app_type: &str,
) -> (serde_json::Value, Option<TriggeredSkill>) {
    let user_text = match extract_user_text(&body) {
        Some(text) => text,
        None => return (body, None),
    };

    let cache = get_skill_trigger_cache();
    let triggered = match cache.detect(&user_text, app_type).await {
        Some(skill) => skill,
        None => return (body, None),
    };

    log::info!(
        "[SkillTrigger] 检测到技能: {} (触发词: '{}')",
        triggered.name,
        triggered.matched_trigger
    );

    // 注入技能内容到系统提示
    let body = inject_skill_content(body, &triggered);

    (body, Some(triggered))
}

/// 注入技能内容到请求体
fn inject_skill_content(
    mut body: serde_json::Value,
    skill: &TriggeredSkill,
) -> serde_json::Value {
    // 构建技能注入提示
    let injection = format!(
        r#"<skill-injection>
<skill-name>{}</skill-name>
<trigger>{}</trigger>

{}
</skill-injection>"#,
        skill.name, skill.matched_trigger, skill.content
    );

    // 注入到系统提示
    if let Some(system) = body.get_mut("system") {
        if let Some(system_str) = system.as_str() {
            *system = serde_json::json!(format!("{}\n\n{}", system_str, injection));
        } else if let Some(system_arr) = system.as_array_mut() {
            // 在数组末尾添加
            system_arr.push(serde_json::json!({
                "type": "text",
                "text": injection
            }));
        }
    } else {
        // 没有系统提示，创建一个
        body["system"] = serde_json::json!(injection);
    }

    body
}

/// 初始化技能触发缓存
pub async fn init_skill_trigger_cache(db: &Arc<Database>) {
    let cache = get_skill_trigger_cache();
    cache.load_from_db(db).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_user_text() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "hello world"}
            ]
        });

        let text = extract_user_text(&body);
        assert_eq!(text, Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_user_text_last() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "response"},
                {"role": "user", "content": "last"}
            ]
        });

        let text = extract_user_text(&body);
        assert_eq!(text, Some("last".to_string()));
    }
}
