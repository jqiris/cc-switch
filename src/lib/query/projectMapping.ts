import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { projectMappingApi } from "@/lib/api/projectMapping";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { ProjectProviderMapping } from "@/types";

/**
 * 获取所有项目映射配置
 */
export function useProjectMappings() {
  return useQuery({
    queryKey: ["projectMappings"],
    queryFn: () => projectMappingApi.getAll(),
  });
}

/**
 * 获取指定应用的项目映射配置
 */
export function useProjectMappingsForApp(appType: string) {
  return useQuery({
    queryKey: ["projectMappings", appType],
    queryFn: () => projectMappingApi.getForApp(appType),
    enabled: !!appType,
  });
}

/**
 * 根据 ID 获取项目映射配置
 */
export function useProjectMappingById(id: string | null) {
  return useQuery({
    queryKey: ["projectMapping", id],
    queryFn: () => projectMappingApi.getById(id!),
    enabled: !!id,
  });
}

/**
 * 保存项目映射配置
 */
export function useSaveProjectMapping() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (mapping: ProjectProviderMapping) =>
      projectMappingApi.save(mapping),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projectMappings"] });
      toast.success(
        t("projectMapping.saveSuccess", { defaultValue: "项目映射已保存" }),
        { closeButton: true },
      );
    },
    onError: (error) => {
      toast.error(
        t("projectMapping.saveFailed", { defaultValue: "保存失败" }) +
          ": " +
          String(error),
      );
    },
  });
}

/**
 * 删除项目映射配置
 */
export function useDeleteProjectMapping() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (id: string) => projectMappingApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projectMappings"] });
      toast.success(
        t("projectMapping.deleteSuccess", { defaultValue: "项目映射已删除" }),
        { closeButton: true },
      );
    },
    onError: (error) => {
      toast.error(
        t("projectMapping.deleteFailed", { defaultValue: "删除失败" }) +
          ": " +
          String(error),
      );
    },
  });
}

/**
 * 刷新 Session 缓存
 */
export function useRefreshSessionCache() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => projectMappingApi.refreshSessionCache(),
    onSuccess: () => {
      toast.success(
        t("projectMapping.cacheRefreshed", {
          defaultValue: "Session 缓存已刷新",
        }),
        { closeButton: true },
      );
    },
    onError: (error) => {
      toast.error(
        t("projectMapping.cacheRefreshFailed", {
          defaultValue: "刷新缓存失败",
        }) +
          ": " +
          String(error),
      );
    },
  });
}

/**
 * 获取 Session 缓存大小
 */
export function useSessionCacheSize() {
  return useQuery({
    queryKey: ["sessionCacheSize"],
    queryFn: () => projectMappingApi.getSessionCacheSize(),
  });
}
