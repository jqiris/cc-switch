import { invoke } from "@tauri-apps/api/core";
import type { ProjectProviderMapping } from "@/types";

export const projectMappingApi = {
  // 获取所有项目映射配置
  async getAll(): Promise<ProjectProviderMapping[]> {
    return invoke("get_all_project_mappings");
  },

  // 获取指定应用的项目映射配置
  async getForApp(appType: string): Promise<ProjectProviderMapping[]> {
    return invoke("get_project_mappings_for_app", { appType });
  },

  // 保存项目映射配置
  async save(mapping: ProjectProviderMapping): Promise<void> {
    return invoke("save_project_mapping", { mapping });
  },

  // 删除项目映射配置
  async delete(id: string): Promise<void> {
    return invoke("delete_project_mapping", { id });
  },

  // 根据 ID 获取项目映射配置
  async getById(id: string): Promise<ProjectProviderMapping | null> {
    return invoke("get_project_mapping_by_id", { id });
  },

  // 刷新 Session 缓存
  async refreshSessionCache(): Promise<void> {
    return invoke("refresh_session_cache");
  },

  // 获取 Session 缓存大小
  async getSessionCacheSize(): Promise<number> {
    return invoke("get_session_cache_size");
  },
};
