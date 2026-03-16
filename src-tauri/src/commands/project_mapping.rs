//! 项目目录映射命令
//!
//! 管理项目目录 → Provider 的映射配置

use crate::database::ProjectProviderMapping;
use crate::store::AppState;

/// 获取所有项目映射配置
#[tauri::command]
pub async fn get_all_project_mappings(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProjectProviderMapping>, String> {
    state
        .db
        .get_all_project_mappings()
        .map_err(|e| e.to_string())
}

/// 获取指定应用的项目映射配置
#[tauri::command]
pub async fn get_project_mappings_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<ProjectProviderMapping>, String> {
    state
        .db
        .get_project_mappings_for_app(&app_type)
        .map_err(|e| e.to_string())
}

/// 保存项目映射配置
#[tauri::command]
pub async fn save_project_mapping(
    state: tauri::State<'_, AppState>,
    mapping: ProjectProviderMapping,
) -> Result<(), String> {
    state
        .db
        .save_project_mapping(&mapping)
        .map_err(|e| e.to_string())?;

    // 保存后自动刷新 SessionCache，确保映射能立即生效
    let cache = crate::services::SessionCache::instance();
    cache.scan_sessions().await;

    Ok(())
}

/// 删除项目映射配置
#[tauri::command]
pub async fn delete_project_mapping(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state
        .db
        .delete_project_mapping(&id)
        .map_err(|e| e.to_string())?;

    // 删除后也刷新 SessionCache，保持状态一致
    let cache = crate::services::SessionCache::instance();
    cache.scan_sessions().await;

    Ok(())
}

/// 根据 ID 获取项目映射配置
#[tauri::command]
pub async fn get_project_mapping_by_id(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<ProjectProviderMapping>, String> {
    state
        .db
        .get_project_mapping_by_id(&id)
        .map_err(|e| e.to_string())
}

/// 刷新 Session 缓存
///
/// 手动触发扫描所有 Session 文件，更新 session_id → cwd 映射
#[tauri::command]
pub async fn refresh_session_cache() -> Result<(), String> {
    let cache = crate::services::SessionCache::instance();
    cache.scan_sessions().await;
    Ok(())
}

/// 获取 Session 缓存大小
#[tauri::command]
pub async fn get_session_cache_size() -> Result<usize, String> {
    let cache = crate::services::SessionCache::instance();
    Ok(cache.size().await)
}
