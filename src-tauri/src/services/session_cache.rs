//! Session 缓存服务
//!
//! 监控 Claude Code Session 文件，提取 session_id 和 cwd 的映射关系，
//! 用于根据项目目录自动选择 Provider。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::config::get_claude_config_dir;

/// Session 元数据缓存
#[derive(Debug, Clone)]
pub struct SessionMeta {
    /// 工作目录
    pub cwd: String,
    /// 最后更新时间
    pub updated_at: Instant,
}

/// Session 缓存服务
///
/// 提供 session_id → cwd 的快速查找，用于项目目录匹配。
/// 采用懒加载策略：首次访问时扫描，后续通过 TTL 刷新。
pub struct SessionCache {
    /// session_id → SessionMeta 映射
    cache: RwLock<HashMap<String, SessionMeta>>,
    /// 缓存过期时间（秒）
    ttl_secs: u64,
    /// 上次全量扫描时间
    last_scan: RwLock<Option<Instant>>,
}

impl SessionCache {
    /// 创建新的 Session 缓存
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl_secs,
            last_scan: RwLock::new(None),
        }
    }

    /// 获取单例实例
    pub fn instance() -> Arc<Self> {
        static INSTANCE: std::sync::OnceLock<Arc<SessionCache>> = std::sync::OnceLock::new();
        INSTANCE
            .get_or_init(|| Arc::new(Self::new(300))) // 默认 5 分钟 TTL
            .clone()
    }

    /// 根据 session_id 获取 cwd
    ///
    /// 如果缓存未命中或已过期，会触发后台扫描。
    pub async fn get_cwd(&self, session_id: &str) -> Option<String> {
        // 先尝试从缓存读取
        {
            let cache = self.cache.read().await;
            if let Some(meta) = cache.get(session_id) {
                // 检查是否过期
                if meta.updated_at.elapsed().as_secs() < self.ttl_secs {
                    log::debug!(
                        "[SessionCache] 缓存命中: session_id={}, cwd={}",
                        session_id,
                        meta.cwd
                    );
                    return Some(meta.cwd.clone());
                }
            }
        }

        // 缓存未命中或过期，触发刷新
        self.refresh_if_needed().await;

        // 再次尝试读取
        let cache = self.cache.read().await;
        if let Some(meta) = cache.get(session_id) {
            log::debug!(
                "[SessionCache] 刷新后命中: session_id={}, cwd={}",
                session_id,
                meta.cwd
            );
            return Some(meta.cwd.clone());
        }

        // 打印一些可用的 session_id 供调试
        let cache = self.cache.read().await;
        let sample_keys: Vec<_> = cache.keys().take(5).collect();
        log::info!(
            "[SessionCache] 未找到 session_id={}，缓存中的示例 keys: {:?}",
            session_id,
            sample_keys
        );
        None
    }

    /// 检查是否需要刷新缓存
    async fn refresh_if_needed(&self) {
        let need_refresh = {
            let last_scan = self.last_scan.read().await;
            match *last_scan {
                None => true,
                Some(instant) => instant.elapsed().as_secs() > self.ttl_secs,
            }
        };

        if need_refresh {
            self.scan_sessions().await;
        }
    }

    /// 扫描所有 Session 文件，更新缓存
    pub async fn scan_sessions(&self) {
        let start = Instant::now();
        let projects_dir = get_claude_config_dir().join("projects");

        if !projects_dir.exists() {
            log::debug!("[SessionCache] 项目目录不存在: {:?}", projects_dir);
            return;
        }

        let mut new_cache = HashMap::new();
        let mut session_count = 0;

        // 递归扫描所有 .jsonl 文件
        let entries: Vec<_> = walkdir::WalkDir::new(&projects_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "jsonl").unwrap_or(false))
            .collect();

        for entry in entries {
            if let Some((session_id, cwd)) = parse_session_file(entry.path()) {
                new_cache.insert(
                    session_id,
                    SessionMeta {
                        cwd,
                        updated_at: Instant::now(),
                    },
                );
                session_count += 1;
            }
        }

        // 更新缓存
        {
            let mut cache = self.cache.write().await;
            *cache = new_cache;
        }

        // 更新扫描时间
        {
            let mut last_scan = self.last_scan.write().await;
            *last_scan = Some(Instant::now());
        }

        log::info!(
            "[SessionCache] 扫描完成: {} 个 session, 耗时 {:?}",
            session_count,
            start.elapsed()
        );
    }

    /// 获取缓存大小
    pub async fn size(&self) -> usize {
        self.cache.read().await.len()
    }
}

/// 解析 Session 文件，提取 session_id 和 cwd
///
/// Session 文件格式：每行一个 JSON 对象
/// 第一行通常包含 sessionId 和 cwd 字段
fn parse_session_file(path: &std::path::Path) -> Option<(String, String)> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    // 只读取前几行（通常元数据在第一行）
    for line in reader.lines().take(5) {
        let line = line.ok()?;
        let value: serde_json::Value = serde_json::from_str(&line).ok()?;

        // 提取 session_id 和 cwd
        let session_id = value
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cwd = value
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let (Some(sid), Some(c)) = (session_id, cwd) {
            return Some((sid, c));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_session_file() {
        let temp = TempDir::new().unwrap();
        let session_path = temp.path().join("test-session.jsonl");

        fs::write(
            &session_path,
            r#"{"sessionId":"abc-123","cwd":"/home/user/projects/test","timestamp":"2024-01-01T00:00:00Z"}
{"message":{"role":"user","content":"hello"},"timestamp":"2024-01-01T00:01:00Z"}
"#,
        )
        .unwrap();

        let result = parse_session_file(&session_path);
        assert_eq!(result, Some(("abc-123".to_string(), "/home/user/projects/test".to_string())));
    }

    #[test]
    fn test_parse_session_file_missing_fields() {
        let temp = TempDir::new().unwrap();
        let session_path = temp.path().join("test-session.jsonl");

        fs::write(
            &session_path,
            r#"{"timestamp":"2024-01-01T00:00:00Z"}
{"message":{"role":"user","content":"hello"}}
"#,
        )
        .unwrap();

        let result = parse_session_file(&session_path);
        assert!(result.is_none());
    }
}
