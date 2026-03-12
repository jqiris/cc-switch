//! 项目目录 → Provider 映射 DAO
//!
//! 支持按项目目录自动选择不同的 Provider（模型配置）

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// 项目目录映射配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProviderMapping {
    /// 唯一标识符
    pub id: String,
    /// 项目目录路径（支持 glob 模式）
    pub project_path: String,
    /// 显示名称（可选，用于 UI 展示）
    pub display_name: Option<String>,
    /// 应用类型（claude, codex, gemini 等）
    pub app_type: String,
    /// 目标 Provider ID
    pub provider_id: String,
    /// 是否启用
    pub enabled: bool,
    /// 优先级（数字越小优先级越高）
    pub priority: i32,
    /// 创建时间
    pub created_at: i64,
    /// 更新时间
    pub updated_at: i64,
}

impl ProjectProviderMapping {
    /// 创建新的映射配置
    pub fn new(
        project_path: String,
        display_name: Option<String>,
        app_type: String,
        provider_id: String,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_path,
            display_name,
            app_type,
            provider_id,
            enabled: true,
            priority: 100,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Database {
    /// 获取所有项目映射配置
    pub fn get_all_project_mappings(&self) -> Result<Vec<ProjectProviderMapping>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, project_path, display_name, app_type, provider_id, enabled, priority, created_at, updated_at
             FROM project_provider_mappings
             ORDER BY priority ASC, created_at DESC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let mappings = stmt.query_map([], |row| {
            Ok(ProjectProviderMapping {
                id: row.get(0)?,
                project_path: row.get(1)?,
                display_name: row.get(2)?,
                app_type: row.get(3)?,
                provider_id: row.get(4)?,
                enabled: row.get(5)?,
                priority: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(mappings)
    }

    /// 获取指定应用的项目映射配置
    pub fn get_project_mappings_for_app(&self, app_type: &str) -> Result<Vec<ProjectProviderMapping>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, project_path, display_name, app_type, provider_id, enabled, priority, created_at, updated_at
             FROM project_provider_mappings
             WHERE app_type = ? AND enabled = 1
             ORDER BY priority ASC, created_at DESC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let mappings = stmt.query_map([app_type], |row| {
            Ok(ProjectProviderMapping {
                id: row.get(0)?,
                project_path: row.get(1)?,
                display_name: row.get(2)?,
                app_type: row.get(3)?,
                provider_id: row.get(4)?,
                enabled: row.get(5)?,
                priority: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(mappings)
    }

    /// 保存项目映射配置（插入或更新）
    pub fn save_project_mapping(&self, mapping: &ProjectProviderMapping) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO project_provider_mappings
             (id, project_path, display_name, app_type, provider_id, enabled, priority, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                mapping.id,
                mapping.project_path,
                mapping.display_name,
                mapping.app_type,
                mapping.provider_id,
                mapping.enabled,
                mapping.priority,
                mapping.created_at,
                mapping.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除项目映射配置
    pub fn delete_project_mapping(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM project_provider_mappings WHERE id = ?",
            params![id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 根据 ID 获取项目映射配置
    pub fn get_project_mapping_by_id(&self, id: &str) -> Result<Option<ProjectProviderMapping>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, project_path, display_name, app_type, provider_id, enabled, priority, created_at, updated_at
             FROM project_provider_mappings WHERE id = ?"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([id], |row| {
            Ok(ProjectProviderMapping {
                id: row.get(0)?,
                project_path: row.get(1)?,
                display_name: row.get(2)?,
                app_type: row.get(3)?,
                provider_id: row.get(4)?,
                enabled: row.get(5)?,
                priority: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        });

        match result {
            Ok(mapping) => Ok(Some(mapping)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 根据项目路径匹配映射配置
    ///
    /// 匹配规则：
    /// 1. 精确匹配优先
    /// 2. 然后按优先级顺序检查前缀匹配
    /// 3. 支持 glob 模式（如 `/home/user/projects/*`）
    pub fn match_project_mapping(
        &self,
        project_path: &str,
        app_type: &str,
    ) -> Result<Option<ProjectProviderMapping>, AppError> {
        let mappings = self.get_project_mappings_for_app(app_type)?;

        log::debug!(
            "[ProjectMapping] 尝试匹配: path={}, app_type={}, 可用映射数={}",
            project_path,
            app_type,
            mappings.len()
        );

        if mappings.is_empty() {
            log::warn!("[ProjectMapping] 没有找到 app_type={} 的启用映射", app_type);
            return Ok(None);
        }

        // 标准化路径（统一使用正斜杠，转小写用于 Windows）
        let normalized_path = project_path.replace('\\', "/").to_lowercase();

        // 1. 尝试精确匹配（支持正斜杠和反斜杠）
        for mapping in &mappings {
            let normalized_config_path = mapping.project_path.replace('\\', "/").to_lowercase();
            if normalized_config_path == normalized_path {
                log::info!(
                    "[ProjectMapping] 精确匹配成功: {} -> provider {}",
                    project_path,
                    mapping.provider_id
                );
                return Ok(Some(mapping.clone()));
            }
        }

        // 2. 尝试 glob 模式匹配
        for mapping in &mappings {
            // 对于 glob，需要转换路径分隔符
            let pattern_str = mapping.project_path.replace('\\', "/");
            if let Ok(pattern) = glob::Pattern::new(&pattern_str) {
                if pattern.matches(&normalized_path) {
                    log::info!(
                        "[ProjectMapping] Glob 匹配成功: {} (pattern={}) -> provider {}",
                        project_path,
                        mapping.project_path,
                        mapping.provider_id
                    );
                    return Ok(Some(mapping.clone()));
                }
            }
        }

        // 3. 尝试前缀匹配（作为后备）
        for mapping in &mappings {
            let normalized_config_path = mapping.project_path.replace('\\', "/").to_lowercase();
            if normalized_path.starts_with(&normalized_config_path) {
                log::info!(
                    "[ProjectMapping] 前缀匹配成功: {} (prefix={}) -> provider {}",
                    project_path,
                    mapping.project_path,
                    mapping.provider_id
                );
                return Ok(Some(mapping.clone()));
            }
        }

        log::debug!(
            "[ProjectMapping] 未找到匹配: path={}, 检查过的映射: {:?}",
            project_path,
            mappings.iter().map(|m| &m.project_path).collect::<Vec<_>>()
        );

        Ok(None)
    }
}
