//! 自动调用模块
//!
//! 参考 oh-my-claudecode 的 auto-invoke.ts 实现
//! 高置信度技能自动调用，使用更强势的注入格式引导 LLM 立即执行
//!
//! ## 核心特性
//! - 置信度阈值检查
//! - 每会话最大调用次数限制
//! - 冷却时间限制
//! - 调用历史记录
//! - 自动调用使用特殊的注入格式（auto_invoke_skill）

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

// =============================================================================
// 类型定义
// =============================================================================

/// 自动调用配置
#[derive(Debug, Clone)]
pub struct AutoInvokeConfig {
    /// 是否启用自动调用
    pub enabled: bool,
    /// 置信度阈值（默认 80%）
    pub confidence_threshold: usize,
    /// 每个会话最大自动调用次数
    pub max_auto_invokes: usize,
    /// 冷却时间（毫秒，默认 30 秒）
    pub cooldown_ms: u64,
}

impl Default for AutoInvokeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            confidence_threshold: 80,
            max_auto_invokes: 3,
            cooldown_ms: 30_000,
        }
    }
}

/// 调用记录
#[derive(Debug, Clone, serde::Serialize)]
pub struct InvocationRecord {
    /// 技能 ID
    pub skill_id: String,
    /// 技能名称
    pub skill_name: String,
    /// 调用时间戳
    pub timestamp_ms: u64,
    /// 置信度
    pub confidence: usize,
    /// 用户提示词摘要
    pub prompt_summary: String,
    /// 是否成功
    pub was_successful: Option<bool>,
}

/// 调用统计
#[derive(Debug, Clone, Default)]
pub struct InvocationStats {
    /// 总调用次数
    pub total: usize,
    /// 成功次数
    pub successful: usize,
    /// 失败次数
    pub failed: usize,
    /// 未知状态次数
    pub unknown: usize,
    /// 平均置信度
    pub average_confidence: f64,
    /// 热门技能列表
    pub top_skills: Vec<SkillStat>,
}

/// 技能统计
#[derive(Debug, Clone)]
pub struct SkillStat {
    pub skill_id: String,
    pub skill_name: String,
    pub count: usize,
    pub success_rate: f64,
}

/// 自动调用状态
struct AutoInvokeSession {
    /// 会话 ID
    session_id: String,
    /// 配置
    config: AutoInvokeConfig,
    /// 调用记录
    invocations: Vec<InvocationRecord>,
    /// 上次调用时间
    last_invoke_ms: u64,
}

// =============================================================================
// 自动调用器
// =============================================================================

/// 自动调用器
pub struct AutoInvoker {
    /// 会话状态：session_id -> state
    sessions: RwLock<HashMap<String, AutoInvokeSession>>,
    /// 全局调用历史（持久化）
    history: RwLock<Vec<InvocationRecord>>,
    /// 存储目录
    storage_dir: PathBuf,
}

impl AutoInvoker {
    /// 创建新的自动调用器
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
            storage_dir,
        }
    }

    /// 判断是否应该自动调用某技能
    pub async fn should_auto_invoke(
        &self,
        session_id: &str,
        skill_id: &str,
        confidence: usize,
    ) -> bool {
        let sessions = self.sessions.read().await;

        let session = match sessions.get(session_id) {
            Some(s) => s,
            None => return false,
        };

        let config = &session.config;

        // 检查是否启用
        if !config.enabled {
            log::trace!(
                "[AutoInvoke] 未启用自动调用"
            );
            return false;
        }

        // 检查置信度阈值
        if confidence < config.confidence_threshold {
            log::debug!(
                "[AutoInvoke] 置信度 {}% 低于阈值 {}%，跳过自动调用: {}",
                confidence, config.confidence_threshold, skill_id
            );
            return false;
        }

        // 检查最大调用次数
        if session.invocations.len() >= config.max_auto_invokes {
            log::debug!(
                "[AutoInvoke] 已达最大调用次数 {}，跳过自动调用: {}",
                config.max_auto_invokes, skill_id
            );
            return false;
        }

        // 检查冷却时间
        let now = current_time_ms();
        if now.saturating_sub(session.last_invoke_ms) < config.cooldown_ms {
            log::debug!(
                "[AutoInvoke] 冷却中，跳过自动调用: {}",
                skill_id
            );
            return false;
        }

        // 检查该技能是否已在本会话调用过
        let already_invoked = session
            .invocations
            .iter()
            .any(|inv| inv.skill_id == skill_id);

        if already_invoked {
            log::debug!(
                "[AutoInvoke] 技能已在本会话调用过，跳过: {}",
                skill_id
            );
            return false;
        }

        log::info!(
            "[AutoInvoke] ✓ 触发自动调用 - 技能: {}, 置信度: {}%",
            skill_id, confidence
        );

        true
    }

    /// 记录一次自动调用
    pub async fn record_invocation(
        &self,
        session_id: &str,
        skill_id: &str,
        skill_name: &str,
        confidence: usize,
        prompt_summary: &str,
    ) {
        let mut sessions = self.sessions.write().await;

        let now = current_time_ms();

        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| AutoInvokeSession {
                session_id: session_id.to_string(),
                config: AutoInvokeConfig::default(),
                invocations: Vec::new(),
                last_invoke_ms: 0,
            });

        let record = InvocationRecord {
            skill_id: skill_id.to_string(),
            skill_name: skill_name.to_string(),
            timestamp_ms: now,
            confidence,
            prompt_summary: prompt_summary.to_string(),
            was_successful: None,
        };

        session.invocations.push(record.clone());
        session.last_invoke_ms = now;

        // 同时记录到全局历史
        let mut history = self.history.write().await;
        history.push(record);
    }

    /// 更新调用结果
    pub async fn update_invocation_result(
        &self,
        session_id: &str,
        skill_id: &str,
        was_successful: bool,
    ) {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            // 找到最近的该技能调用记录
            if let Some(record) = session
                .invocations
                .iter_mut()
                .rev()
                .find(|inv| inv.skill_id == skill_id)
            {
                record.was_successful = Some(was_successful);
                log::info!(
                    "[AutoInvoke] 调用结果更新 - 技能: {}, 成功: {}",
                    skill_id, was_successful
                );
            }
        }

        // 同步更新全局历史
        let mut history = self.history.write().await;
        if let Some(record) = history
            .iter_mut()
            .rev()
            .find(|inv| inv.skill_id == skill_id)
        {
            record.was_successful = Some(was_successful);
        }
    }

    /// 获取会话统计
    pub async fn get_session_stats(&self, session_id: &str) -> InvocationStats {
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(session_id) {
            calculate_stats(&session.invocations)
        } else {
            InvocationStats::default()
        }
    }

    /// 获取全局统计
    pub async fn get_global_stats(&self) -> InvocationStats {
        let history = self.history.read().await;
        calculate_stats(&history)
    }

    /// 获取会话的自动调用数量
    pub async fn session_invoke_count(&self, session_id: &str) -> usize {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|s| s.invocations.len())
            .unwrap_or(0)
    }

    /// 保存调用历史到磁盘
    pub async fn save_history(&self) {
        let history = self.history.read().await;

        if history.is_empty() {
            return;
        }

        let history_dir = self.storage_dir.join("analytics").join("invocations");

        // 确保目录存在
        if let Err(e) = std::fs::create_dir_all(&history_dir) {
            log::warn!("[AutoInvoke] 无法创建目录 {:?}: {}", history_dir, e);
            return;
        }

        // 按会话分组
        let mut session_records: HashMap<String, Vec<&InvocationRecord>> = HashMap::new();
        for record in history.iter() {
            // 用 timestamp 的日期部分作为 session 标识
            let date_key = format_date_key(record.timestamp_ms);
            session_records
                .entry(date_key)
                .or_default()
                .push(record);
        }

        let mut saved_count = 0;
        for (date_key, records) in &session_records {
            let stats = calculate_stats_from_refs(records);

            let data = serde_json::json!({
                "date": date_key,
                "invocations": records,
                "stats": {
                    "total": stats.total,
                    "successful": stats.successful,
                    "failed": stats.failed,
                    "average_confidence": stats.average_confidence,
                },
                "saved_at": current_time_ms(),
            });

            let file_path = history_dir.join(format!("{}.json", date_key));
            match serde_json::to_string_pretty(&data) {
                Ok(json_str) => {
                    if let Err(e) = std::fs::write(&file_path, &json_str) {
                        log::warn!("[AutoInvoke] 无法保存历史文件 {:?}: {}", file_path, e);
                    } else {
                        saved_count += 1;
                    }
                }
                Err(e) => {
                    log::warn!("[AutoInvoke] 序列化失败: {}", e);
                }
            }
        }

        log::debug!("[AutoInvoke] 已保存 {} 个历史文件", saved_count);
    }

    /// 清理过期数据
    pub async fn cleanup(&self, max_age_days: u64) {
        let mut history = self.history.write().await;
        let cutoff = current_time_ms() - (max_age_days * 24 * 3600 * 1000);

        let before = history.len();
        history.retain(|r| r.timestamp_ms > cutoff);

        let removed = before - history.len();
        if removed > 0 {
            log::info!(
                "[AutoInvoke] 清理了 {} 条过期记录（超过 {} 天）",
                removed, max_age_days
            );
        }

        // 清理会话状态
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, session| {
            if let Some(last) = session.invocations.last() {
                last.timestamp_ms > cutoff
            } else {
                false
            }
        });
    }
}

// =============================================================================
// 自动调用注入格式
// =============================================================================

/// 格式化自动调用注入内容
///
/// 与普通注入不同，自动调用使用更强势的格式，引导 LLM 立即执行技能指令
pub fn format_auto_invoke(skill_name: &str, content: &str, confidence: usize) -> String {
    format!(
        r#"<auto_invoke_skill>
HIGH CONFIDENCE MATCH ({confidence}%) - AUTO-INVOKING SKILL

SKILL: {skill_name}
CONFIDENCE: {confidence}%
STATUS: AUTOMATICALLY INVOKED

{content}

INSTRUCTION: This skill has been automatically invoked due to high confidence match.
Please follow the skill's instructions immediately.
</auto_invoke_skill>"#,
        skill_name = skill_name,
        confidence = confidence,
        content = content,
    )
}

/// 判断注入内容是否为自动调用格式
pub fn is_auto_invoke_injection(content: &str) -> bool {
    content.contains("<auto_invoke_skill>")
        && content.contains("AUTOMATICALLY INVOKED")
}

// =============================================================================
// 辅助函数
// =============================================================================

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn format_date_key(timestamp_ms: u64) -> String {
    let secs = (timestamp_ms / 1000) as i64;
    // 简单的日期格式 YYYY-MM-DD
    let days = secs / 86400;
    let _epoch_day = 1970 - 01 - 01;

    // 简化实现
    format!("day-{}", days)
}

fn calculate_stats(records: &[InvocationRecord]) -> InvocationStats {
    let mut stats = InvocationStats::default();
    stats.total = records.len();

    for record in records {
        match record.was_successful {
            Some(true) => stats.successful += 1,
            Some(false) => stats.failed += 1,
            None => stats.unknown += 1,
        }
    }

    if stats.total > 0 {
        stats.average_confidence =
            records.iter().map(|r| r.confidence as f64).sum::<f64>() / stats.total as f64;
    }

    // 计算热门技能
    let mut skill_counts: HashMap<String, (String, usize, usize)> = HashMap::new();
    for record in records {
        let entry = skill_counts
            .entry(record.skill_id.clone())
            .or_insert_with(|| (record.skill_name.clone(), 0, 0));
        entry.1 += 1; // total
        if record.was_successful == Some(true) {
            entry.2 += 1; // successful
        }
    }

    let mut top_skills: Vec<SkillStat> = skill_counts
        .into_iter()
        .map(|(id, (name, total, successful))| SkillStat {
            skill_id: id,
            skill_name: name,
            count: total,
            success_rate: if total > 0 {
                (successful as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect();

    top_skills.sort_by(|a, b| b.count.cmp(&a.count));
    top_skills.truncate(10);
    stats.top_skills = top_skills;

    stats
}

fn calculate_stats_from_refs(records: &[&InvocationRecord]) -> InvocationStats {
    let mut stats = InvocationStats::default();
    stats.total = records.len();

    for record in records {
        match record.was_successful {
            Some(true) => stats.successful += 1,
            Some(false) => stats.failed += 1,
            None => stats.unknown += 1,
        }
    }

    if stats.total > 0 {
        stats.average_confidence =
            records.iter().map(|r| r.confidence as f64).sum::<f64>() / stats.total as f64;
    }

    stats
}

// =============================================================================
// 测试
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> std::path::PathBuf {
        std::env::temp_dir().join("cc-switch-test-auto-invoke")
    }

    fn create_invoker() -> AutoInvoker {
        let dir = test_dir();
        let _ = std::fs::create_dir_all(&dir);
        AutoInvoker::new(dir)
    }

    // ---- format_auto_invoke / is_auto_invoke_injection ----

    #[test]
    fn test_format_auto_invoke_contains_required_tags() {
        let result = format_auto_invoke("TestSkill", "do something", 95);
        assert!(result.contains("<auto_invoke_skill>"));
        assert!(result.contains("</auto_invoke_skill>"));
        assert!(result.contains("TestSkill"));
        assert!(result.contains("95%"));
        assert!(result.contains("AUTOMATICALLY INVOKED"));
        assert!(result.contains("do something"));
    }

    #[test]
    fn test_is_auto_invoke_injection_positive() {
        let content = format_auto_invoke("Skill", "body", 80);
        assert!(is_auto_invoke_injection(&content));
    }

    #[test]
    fn test_is_auto_invoke_injection_negative() {
        assert!(!is_auto_invoke_injection("plain text"));
        assert!(!is_auto_invoke_injection("<skill-injection>normal</skill-injection>"));
    }

    // ---- format_date_key ----

    #[test]
    fn test_format_date_key_deterministic() {
        let key1 = format_date_key(1_700_000_000_000);
        let key2 = format_date_key(1_700_000_000_000);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_format_date_key_different_days() {
        let day_a = format_date_key(0);
        let day_b = format_date_key(86_400_000); // +1 day in ms
        assert_ne!(day_a, day_b);
    }

    // ---- calculate_stats ----

    #[test]
    fn test_calculate_stats_empty() {
        let stats = calculate_stats(&[]);
        assert_eq!(stats.total, 0);
        assert_eq!(stats.successful, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.unknown, 0);
        assert_eq!(stats.average_confidence, 0.0);
        assert!(stats.top_skills.is_empty());
    }

    #[test]
    fn test_calculate_stats_mixed_results() {
        let records = vec![
            InvocationRecord {
                skill_id: "s1".into(),
                skill_name: "Skill1".into(),
                timestamp_ms: 1000,
                confidence: 90,
                prompt_summary: "test".into(),
                was_successful: Some(true),
            },
            InvocationRecord {
                skill_id: "s1".into(),
                skill_name: "Skill1".into(),
                timestamp_ms: 2000,
                confidence: 80,
                prompt_summary: "test".into(),
                was_successful: Some(false),
            },
            InvocationRecord {
                skill_id: "s2".into(),
                skill_name: "Skill2".into(),
                timestamp_ms: 3000,
                confidence: 70,
                prompt_summary: "test".into(),
                was_successful: None,
            },
        ];
        let stats = calculate_stats(&records);
        assert_eq!(stats.total, 3);
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.unknown, 1);
        assert!((stats.average_confidence - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_stats_top_skills_sorted_by_count() {
        let records = vec![
            InvocationRecord {
                skill_id: "a".into(), skill_name: "A".into(), timestamp_ms: 1,
                confidence: 50, prompt_summary: "x".into(), was_successful: Some(true),
            },
            InvocationRecord {
                skill_id: "a".into(), skill_name: "A".into(), timestamp_ms: 2,
                confidence: 60, prompt_summary: "x".into(), was_successful: Some(true),
            },
            InvocationRecord {
                skill_id: "b".into(), skill_name: "B".into(), timestamp_ms: 3,
                confidence: 70, prompt_summary: "x".into(), was_successful: Some(false),
            },
        ];
        let stats = calculate_stats(&records);
        assert_eq!(stats.top_skills.len(), 2);
        assert_eq!(stats.top_skills[0].skill_id, "a");
        assert_eq!(stats.top_skills[0].count, 2);
        assert_eq!(stats.top_skills[1].skill_id, "b");
    }

    // ---- 异步测试：AutoInvoker 核心流程 ----

    #[tokio::test]
    async fn test_should_auto_invoke_no_session_returns_false() {
        let invoker = create_invoker();
        assert!(!invoker.should_auto_invoke("nonexistent", "skill-1", 95).await);
    }

    #[tokio::test]
    async fn test_should_auto_invoke_low_confidence_rejected() {
        let invoker = create_invoker();
        // 先记录一次调用以创建 session
        invoker.record_invocation("sess-1", "skill-1", "Skill1", 90, "test").await;
        // 低置信度应被拒绝
        assert!(!invoker.should_auto_invoke("sess-1", "skill-2", 50).await);
    }

    #[tokio::test]
    async fn test_should_auto_invoke_duplicate_skill_rejected() {
        let invoker = create_invoker();
        invoker.record_invocation("sess-dup", "skill-x", "X", 95, "test").await;
        // 同一技能不能再次自动调用
        assert!(!invoker.should_auto_invoke("sess-dup", "skill-x", 95).await);
    }

    #[tokio::test]
    async fn test_max_invokes_limit() {
        let invoker = create_invoker();
        invoker.record_invocation("sess-max", "s1", "S1", 90, "t").await;
        invoker.record_invocation("sess-max", "s2", "S2", 90, "t").await;
        invoker.record_invocation("sess-max", "s3", "S3", 90, "t").await;

        // 默认 max_auto_invokes = 3，第4个应被拒绝
        assert!(!invoker.should_auto_invoke("sess-max", "s4", 90).await);
    }

    #[tokio::test]
    async fn test_record_invocation_creates_session() {
        let invoker = create_invoker();
        invoker.record_invocation("new-sess", "sk1", "SK1", 85, "prompt").await;

        assert_eq!(invoker.session_invoke_count("new-sess").await, 1);
    }

    #[tokio::test]
    async fn test_update_invocation_result() {
        let invoker = create_invoker();
        invoker.record_invocation("sess-res", "sk1", "SK1", 85, "prompt").await;
        invoker.update_invocation_result("sess-res", "sk1", true).await;

        let stats = invoker.get_session_stats("sess-res").await;
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.failed, 0);
    }

    #[tokio::test]
    async fn test_global_stats_across_sessions() {
        let invoker = create_invoker();
        invoker.record_invocation("sess-a", "sk1", "S1", 80, "p").await;
        invoker.record_invocation("sess-b", "sk2", "S2", 90, "p").await;

        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 2);
    }

    #[tokio::test]
    async fn test_get_session_stats_nonexistent() {
        let invoker = create_invoker();
        let stats = invoker.get_session_stats("no-such-session").await;
        assert_eq!(stats.total, 0);
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_records() {
        let invoker = create_invoker();
        // 记录一些调用
        invoker.record_invocation("sess-old", "sk1", "S1", 80, "p").await;
        // 清理 0 天前的记录（即全部清理）
        invoker.cleanup(0).await;

        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 0);
    }

    #[tokio::test]
    async fn test_save_history_writes_files() {
        let invoker = create_invoker();
        invoker.record_invocation("sess-save", "sk1", "S1", 85, "test prompt").await;
        invoker.save_history().await;

        // 验证文件已写入
        let history_dir = test_dir().join("analytics").join("invocations");
        assert!(history_dir.exists());

        let entries: Vec<_> = std::fs::read_dir(&history_dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .collect();
        assert!(!entries.is_empty(), "应至少生成一个历史文件");

        // 清理
        let _ = std::fs::remove_dir_all(test_dir());
    }

    // ---- 更多边界和覆盖测试 ----

    #[test]
    fn test_auto_invoke_config_default() {
        let config = AutoInvokeConfig::default();
        assert!(config.enabled);
        assert_eq!(config.confidence_threshold, 80);
        assert_eq!(config.max_auto_invokes, 3);
        assert_eq!(config.cooldown_ms, 30_000);
    }

    #[test]
    fn test_format_auto_invoke_content_escaping() {
        // 测试内容中有特殊字符的情况
        let result = format_auto_invoke("Test", "Content with <tags> and \"quotes\"", 95);
        assert!(result.contains("Content with <tags>"));
        assert!(result.contains("Test"));
        assert!(result.contains("95%"));
    }

    #[test]
    fn test_format_auto_invoke_empty_content() {
        let result = format_auto_invoke("Skill", "", 50);
        assert!(result.contains("<auto_invoke_skill>"));
        assert!(result.contains("Skill"));
        assert!(result.contains("50%"));
    }

    #[test]
    fn test_format_auto_invoke_confidence_100() {
        let result = format_auto_invoke("Skill", "content", 100);
        assert!(result.contains("100%"));
    }

    #[test]
    fn test_format_auto_invoke_confidence_0() {
        let result = format_auto_invoke("Skill", "content", 0);
        assert!(result.contains("0%"));
    }

    #[test]
    fn test_is_auto_invoke_injection_partial_match() {
        // 部分匹配应返回 false
        assert!(!is_auto_invoke_injection("<auto_invoke"));
        assert!(!is_auto_invoke_injection("AUTOMATICALLY INVOKED"));
        // 必须同时包含两个标记
        assert!(is_auto_invoke_injection("<auto_invoke_skill>AUTOMATICALLY INVOKED</auto_invoke_skill>"));
    }

    #[tokio::test]
    async fn test_should_auto_invoke_when_disabled() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t").await;

        // 禁用自动调用后应返回 false
        let mut sessions = invoker.sessions.write().await;
        if let Some(sess) = sessions.get_mut("sess") {
            sess.config.enabled = false;
        }
        drop(sessions);

        assert!(!invoker.should_auto_invoke("sess", "sk2", 95).await);
    }

    #[tokio::test]
    async fn test_should_auto_invoke_below_threshold() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t").await;

        // 修改阈值
        let mut sessions = invoker.sessions.write().await;
        if let Some(sess) = sessions.get_mut("sess") {
            sess.config.confidence_threshold = 95;
        }
        drop(sessions);

        assert!(!invoker.should_auto_invoke("sess", "sk2", 90).await);
    }

    #[tokio::test]
    async fn test_should_auto_invoke_cooldown() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t").await;

        // 设置非常长的冷却时间
        let mut sessions = invoker.sessions.write().await;
        if let Some(sess) = sessions.get_mut("sess") {
            sess.config.cooldown_ms = 999_999_999;
            sess.last_invoke_ms = current_time_ms();
        }
        drop(sessions);

        assert!(!invoker.should_auto_invoke("sess", "sk2", 95).await);
    }

    #[tokio::test]
    async fn test_record_invocation_updates_last_invoke() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t").await;

        let sessions = invoker.sessions.read().await;
        let sess = sessions.get("sess").unwrap();
        assert!(sess.last_invoke_ms > 0);
    }

    #[tokio::test]
    async fn test_record_invocation_same_skill_twice() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t1").await;
        invoker.record_invocation("sess", "sk1", "S1", 85, "t2").await;

        let sessions = invoker.sessions.read().await;
        let sess = sessions.get("sess").unwrap();
        // 可以记录同一技能多次
        assert_eq!(sess.invocations.len(), 2);
    }

    #[tokio::test]
    async fn test_update_invocation_result_no_session() {
        let invoker = create_invoker();
        // 不存在的会话不应 panic
        invoker.update_invocation_result("nosess", "sk1", true).await;
    }

    #[tokio::test]
    async fn test_get_session_stats_empty() {
        let invoker = create_invoker();
        let stats = invoker.get_session_stats("empty").await;
        assert_eq!(stats.total, 0);
        assert_eq!(stats.successful, 0);
        assert_eq!(stats.failed, 0);
    }

    #[tokio::test]
    async fn test_get_global_stats_empty() {
        let invoker = create_invoker();
        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 0);
        assert!(stats.top_skills.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_with_no_records() {
        let invoker = create_invoker();
        // 空记录不应 panic
        invoker.cleanup(30).await;
        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn test_current_time_ms_increasing() {
        let t1 = current_time_ms();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let t2 = current_time_ms();
        assert!(t2 > t1);
    }

    #[test]
    fn test_format_date_key_monotonic() {
        // 后续日期的 key 应该不同
        let k1 = format_date_key(0);
        let k2 = format_date_key(86_400_000); // +1 day
        assert_ne!(k1, k2);
    }

    #[tokio::test]
    async fn test_session_invoke_count_no_session() {
        let invoker = create_invoker();
        assert_eq!(invoker.session_invoke_count("nosess").await, 0);
    }

    #[tokio::test]
    async fn test_save_history_with_empty_records() {
        let invoker = create_invoker();
        // 空历史不应 panic
        invoker.save_history().await;
    }

    #[tokio::test]
    async fn test_multiple_sessions_independent() {
        let invoker = create_invoker();
        invoker.record_invocation("sess1", "sk1", "S1", 90, "t").await;
        invoker.record_invocation("sess2", "sk2", "S2", 85, "t").await;

        let stats1 = invoker.get_session_stats("sess1").await;
        let stats2 = invoker.get_session_stats("sess2").await;

        assert_eq!(stats1.total, 1);
        assert_eq!(stats2.total, 1);
    }

    #[tokio::test]
    async fn test_global_stats_aggregates_sessions() {
        let invoker = create_invoker();
        invoker.record_invocation("sess1", "sk1", "S1", 90, "t").await;
        invoker.record_invocation("sess2", "sk2", "S2", 85, "t").await;
        invoker.record_invocation("sess1", "sk3", "S3", 80, "t").await;

        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 3);
        // 平均置信度应在 80-90 之间
        assert!(stats.average_confidence >= 80.0 && stats.average_confidence <= 90.0);
    }

    #[test]
    fn test_invocation_record_serialize() {
        let record = InvocationRecord {
            skill_id: "test".to_string(),
            skill_name: "Test".to_string(),
            timestamp_ms: 12345,
            confidence: 90,
            prompt_summary: "summary".to_string(),
            was_successful: Some(true),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("90"));
    }

    #[test]
    fn test_skill_stat_success_rate_calculation() {
        let stat = SkillStat {
            skill_id: "s1".to_string(),
            skill_name: "Skill1".to_string(),
            count: 10,
            success_rate: 70.0,
        };
        assert_eq!(stat.skill_id, "s1");
        assert_eq!(stat.count, 10);
        assert_eq!(stat.success_rate, 70.0);
    }

    #[tokio::test]
    async fn test_cleanup_preserves_recent() {
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 90, "t").await;

        // 清理 30 天前的记录，当前记录应保留
        invoker.cleanup(30).await;

        let stats = invoker.get_global_stats().await;
        assert_eq!(stats.total, 1);
    }

    #[tokio::test]
    async fn test_mark_injected_then_check() {
        // 这与 skill_trigger 模块集成，但我们可以测试基本流程
        let invoker = create_invoker();
        invoker.record_invocation("sess", "sk1", "S1", 95, "test").await;

        let count = invoker.session_invoke_count("sess").await;
        assert_eq!(count, 1);
    }
}
