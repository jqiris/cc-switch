/**
 * 项目目录映射管理组件
 *
 * 允许用户配置项目目录 → Provider 的自动映射规则：
 * - 添加/编辑/删除映射配置
 * - 支持精确路径、glob 模式匹配
 * - 支持启用/禁用映射
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FolderTree,
  FolderOpen,
  Plus,
  Trash2,
  Loader2,
  Edit2,
  RefreshCw,
  Info,
  Globe,
  ToggleLeft,
  ToggleRight,
  Route,
  Zap,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { settingsApi } from "@/lib/api/settings";
import { Switch } from "@/components/ui/switch";
import { Slider } from "@/components/ui/slider";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type { ProjectProviderMapping } from "@/types";
import type { AppId } from "@/lib/api";
import {
  useProjectMappings,
  useSaveProjectMapping,
  useDeleteProjectMapping,
  useRefreshSessionCache,
  useSessionCacheSize,
} from "@/lib/query/projectMapping";
import { useProvidersQuery } from "@/lib/query/queries";

interface ProjectMappingPanelProps {
  appType: AppId;
  disabled?: boolean;
}

export function ProjectMappingPanel({
  appType,
  disabled = false,
}: ProjectMappingPanelProps) {
  const { t } = useTranslation();

  // 查询数据
  const { data: mappings = [], isLoading: isMappingsLoading } =
    useProjectMappings();
  const { data: providersData, isLoading: isProvidersLoading } =
    useProvidersQuery(appType);
  const { data: cacheSize = 0 } = useSessionCacheSize();

  // 提取 providers 对象
  const providers = providersData?.providers;

  // Mutations
  const saveMapping = useSaveProjectMapping();
  const deleteMapping = useDeleteProjectMapping();
  const refreshCache = useRefreshSessionCache();

  // 编辑弹窗状态
  const [editDialogOpen, setEditDialogOpen] = useState(false);
  const [editingMapping, setEditingMapping] =
    useState<ProjectProviderMapping | null>(null);

  // 表单状态
  const [formData, setFormData] = useState<{
    projectPath: string;
    displayName: string;
    providerId: string;
    enabled: boolean;
    priority: number;
  }>({
    projectPath: "",
    displayName: "",
    providerId: "",
    enabled: true,
    priority: 100,
  });

  // 过滤当前应用的映射
  const filteredMappings = mappings.filter((m) => m.appType === appType);

  // 获取 Provider 名称
  const getProviderName = (providerId: string): string => {
    if (!providers) return providerId;
    return providers[providerId]?.name || providerId;
  };

  // 检查 Provider 是否存在
  const isProviderMissing = (providerId: string): boolean => {
    if (!providers) return true;
    return !providers[providerId];
  };

  // 打开新增弹窗
  const handleAdd = () => {
    setEditingMapping(null);
    setFormData({
      projectPath: "",
      displayName: "",
      providerId: "",
      enabled: true,
      priority: 100,
    });
    setEditDialogOpen(true);
  };

  // 打开编辑弹窗
  const handleEdit = (mapping: ProjectProviderMapping) => {
    setEditingMapping(mapping);
    setFormData({
      projectPath: mapping.projectPath,
      displayName: mapping.displayName || "",
      providerId: mapping.providerId,
      enabled: mapping.enabled,
      priority: mapping.priority,
    });
    setEditDialogOpen(true);
  };

  // 保存映射
  const handleSave = async () => {
    if (!formData.projectPath.trim() || !formData.providerId) {
      return;
    }

    const now = Date.now();
    const mapping: ProjectProviderMapping = {
      id: editingMapping?.id || crypto.randomUUID(),
      projectPath: formData.projectPath.trim(),
      displayName: formData.displayName.trim() || undefined,
      appType,
      providerId: formData.providerId,
      enabled: formData.enabled,
      priority: formData.priority,
      createdAt: editingMapping?.createdAt || now,
      updatedAt: now,
    };

    await saveMapping.mutateAsync(mapping);
    setEditDialogOpen(false);
  };

  // 删除映射
  const handleDelete = async (id: string) => {
    await deleteMapping.mutateAsync(id);
  };

  // 切换启用状态
  const handleToggleEnabled = async (mapping: ProjectProviderMapping) => {
    const updated: ProjectProviderMapping = {
      ...mapping,
      enabled: !mapping.enabled,
      updatedAt: Date.now(),
    };
    await saveMapping.mutateAsync(updated);
  };

  // 刷新缓存
  const handleRefreshCache = async () => {
    await refreshCache.mutateAsync();
  };

  if (isMappingsLoading || isProvidersLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-5">
      {/* 说明信息 */}
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t(
            "projectMapping.info",
            "配置项目目录与供应商的自动映射。当代理检测到请求来自特定项目目录时，将自动使用对应的供应商。",
          )}
        </AlertDescription>
      </Alert>

      {/* Session 缓存状态 */}
      <div className="flex items-center justify-between p-4 rounded-lg bg-gradient-to-r from-muted/80 to-muted/40 border border-border/50">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-blue-500/15">
            <Globe className="h-5 w-5 text-blue-500" />
          </div>
          <div>
            <p className="text-sm font-medium">
              {t("projectMapping.sessionCache", "Session 缓存")}
            </p>
            <p className="text-xs text-muted-foreground">
              <span className="font-semibold text-foreground">{cacheSize}</span>{" "}
              {t("projectMapping.sessions", "个会话")}
            </p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={handleRefreshCache}
          disabled={refreshCache.isPending}
          className="gap-2"
        >
          {refreshCache.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
          {t("common.refresh", "刷新")}
        </Button>
      </div>

      {/* 映射列表区域 */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h4 className="text-sm font-semibold">
              {t("projectMapping.mappingList", "映射列表")}
            </h4>
            <p className="text-xs text-muted-foreground mt-0.5">
              {t(
                "projectMapping.mappingListHint",
                "按优先级排序，数字越小优先级越高",
              )}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={handleAdd}
            disabled={disabled}
            className="gap-1.5"
          >
            <Plus className="h-4 w-4" />
            {t("projectMapping.addMapping", "添加映射")}
          </Button>
        </div>

        {/* 映射列表 */}
        {filteredMappings.length === 0 ? (
          <div className="rounded-lg border border-dashed border-muted-foreground/40 p-8 text-center">
            <FolderTree className="h-10 w-10 mx-auto mb-3 text-muted-foreground/50" />
            <p className="text-sm text-muted-foreground">
              {t(
                "projectMapping.empty",
                "暂无映射配置。添加映射后，系统将根据项目目录自动选择供应商。",
              )}
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {filteredMappings
              .sort((a, b) => a.priority - b.priority)
              .map((mapping) => (
                <MappingItem
                  key={mapping.id}
                  mapping={mapping}
                  providerName={getProviderName(mapping.providerId)}
                  providerMissing={isProviderMissing(mapping.providerId)}
                  disabled={disabled}
                  onEdit={() => handleEdit(mapping)}
                  onDelete={() => handleDelete(mapping.id)}
                  onToggleEnabled={() => handleToggleEnabled(mapping)}
                  isSaving={saveMapping.isPending || deleteMapping.isPending}
                />
              ))}
          </div>
        )}
      </div>

      {/* 编辑弹窗 */}
      <Dialog open={editDialogOpen} onOpenChange={setEditDialogOpen}>
        <DialogContent className="sm:max-w-[540px] p-0 gap-0">
          {/* Header with gradient background */}
          <div className="relative px-6 pt-6 pb-4 border-b border-border/50">
            <div className="absolute inset-0 bg-gradient-to-r from-blue-500/5 via-transparent to-purple-500/5" />
            <div className="relative flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-blue-500/10 border border-blue-500/20">
                <Route className="h-5 w-5 text-blue-500" />
              </div>
              <div>
                <DialogTitle className="text-lg font-semibold">
                  {editingMapping
                    ? t("projectMapping.editMapping", "编辑映射")
                    : t("projectMapping.addMapping", "添加映射")}
                </DialogTitle>
                <DialogDescription className="text-xs text-muted-foreground mt-0.5">
                  {editingMapping
                    ? t(
                        "projectMapping.editMappingHint",
                        "修改项目目录与供应商的映射配置",
                      )
                    : t(
                        "projectMapping.addMappingHint",
                        "创建新的项目目录映射规则",
                      )}
                </DialogDescription>
              </div>
            </div>
          </div>

          {/* Form content */}
          <div className="px-6 py-5 space-y-5">
            {/* 必填字段区块 */}
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground uppercase tracking-wide">
                <span className="flex h-1.5 w-1.5 rounded-full bg-blue-500" />
                {t("projectMapping.requiredFields", "必填信息")}
              </div>

              {/* 项目路径 */}
              <div className="space-y-2">
                <Label htmlFor="projectPath" className="text-sm font-medium">
                  {t("projectMapping.projectPath", "项目路径")}
                </Label>
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <FolderTree className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      id="projectPath"
                      value={formData.projectPath}
                      readOnly
                      placeholder={t(
                        "projectMapping.projectPathPlaceholder",
                        "点击右侧按钮选择项目目录",
                      )}
                      className={cn(
                        "pl-10 pr-3 cursor-default bg-muted/30",
                        formData.projectPath.trim() &&
                          "border-emerald-500/50 focus-visible:border-emerald-500",
                      )}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="gap-1.5 shrink-0"
                    onClick={async () => {
                      const selected = await settingsApi.selectConfigDirectory(
                        formData.projectPath || undefined,
                      );
                      if (selected) {
                        setFormData({ ...formData, projectPath: selected });
                      }
                    }}
                  >
                    <FolderOpen className="h-4 w-4" />
                    {t("projectMapping.selectDir", "选择目录")}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground flex items-center gap-1">
                  <Info className="h-3 w-3" />
                  {t(
                    "projectMapping.projectPathHint",
                    "通过目录选择器选取项目根目录，避免手动输入导致路径格式错误",
                  )}
                </p>
              </div>

              {/* 选择供应商 */}
              <div className="space-y-2">
                <Label htmlFor="providerId" className="text-sm font-medium">
                  {t("projectMapping.provider", "供应商")}
                </Label>
                <Select
                  value={formData.providerId}
                  onValueChange={(value) =>
                    setFormData({ ...formData, providerId: value })
                  }
                >
                  <SelectTrigger
                    className={cn(
                      "transition-colors",
                      formData.providerId &&
                        "border-emerald-500/50 focus:border-emerald-500",
                    )}
                  >
                    <SelectValue
                      placeholder={t(
                        "projectMapping.selectProvider",
                        "选择供应商",
                      )}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {providers &&
                      Object.entries(providers).map(([id, provider]) => (
                        <SelectItem key={id} value={id}>
                          {provider.name}
                        </SelectItem>
                      ))}
                    {(!providers || Object.keys(providers).length === 0) && (
                      <div className="px-2 py-4 text-center text-sm text-muted-foreground">
                        {t(
                          "projectMapping.noProviders",
                          "没有可用的供应商",
                        )}
                      </div>
                    )}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {/* 分隔线 */}
            <div className="border-t border-border/50" />

            {/* 可选配置区块 */}
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground uppercase tracking-wide">
                <span className="flex h-1.5 w-1.5 rounded-full bg-muted-foreground/50" />
                {t("projectMapping.optionalFields", "可选配置")}
              </div>

              {/* 显示名称 */}
              <div className="space-y-2">
                <Label htmlFor="displayName" className="text-sm font-medium">
                  {t("projectMapping.displayName", "显示名称")}
                  <span className="text-muted-foreground text-xs font-normal ml-1.5">
                    ({t("common.optional", "可选")})
                  </span>
                </Label>
                <Input
                  id="displayName"
                  value={formData.displayName}
                  onChange={(e) =>
                    setFormData({ ...formData, displayName: e.target.value })
                  }
                  placeholder={t(
                    "projectMapping.displayNamePlaceholder",
                    "我的项目",
                  )}
                />
              </div>

              {/* 优先级滑块 */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <Label htmlFor="priority" className="text-sm font-medium">
                    {t("projectMapping.priority", "优先级")}
                  </Label>
                  <div
                    className={cn(
                      "px-2.5 py-1 rounded-md text-sm font-semibold tabular-nums",
                      formData.priority <= 50
                        ? "bg-orange-500/15 text-orange-600 dark:text-orange-400"
                        : formData.priority <= 100
                          ? "bg-blue-500/15 text-blue-600 dark:text-blue-400"
                          : "bg-muted text-muted-foreground",
                    )}
                  >
                    {formData.priority}
                  </div>
                </div>
                <Slider
                  id="priority"
                  value={[formData.priority]}
                  onValueChange={([value]) =>
                    setFormData({ ...formData, priority: value })
                  }
                  min={1}
                  max={200}
                  step={1}
                  className="py-2"
                />
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span className="flex items-center gap-1">
                    <Zap className="h-3 w-3 text-orange-500" />
                    {t("projectMapping.priorityHigh", "高")}
                  </span>
                  <span>{t("projectMapping.priorityMedium", "中")}</span>
                  <span>{t("projectMapping.priorityLow", "低")}</span>
                </div>
              </div>

              {/* 启用开关 */}
              <div className="flex items-center justify-between p-3 rounded-lg bg-muted/30 border border-border/50">
                <div className="space-y-0.5">
                  <Label htmlFor="enabled" className="text-sm font-medium">
                    {t("projectMapping.enabled", "启用映射")}
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    {t(
                      "projectMapping.enabledHint",
                      "禁用后该映射规则将不会生效",
                    )}
                  </p>
                </div>
                <Switch
                  id="enabled"
                  checked={formData.enabled}
                  onCheckedChange={(checked) =>
                    setFormData({ ...formData, enabled: checked })
                  }
                />
              </div>
            </div>
          </div>

          {/* Footer */}
          <div className="px-6 py-4 border-t border-border/50 bg-muted/20">
            <DialogFooter className="gap-2 sm:gap-2">
              <Button
                variant="outline"
                onClick={() => setEditDialogOpen(false)}
                className="flex-1 sm:flex-none"
              >
                {t("common.cancel", "取消")}
              </Button>
              <Button
                onClick={handleSave}
                disabled={
                  !formData.projectPath.trim() ||
                  !formData.providerId ||
                  saveMapping.isPending
                }
                className="flex-1 sm:flex-none min-w-[100px]"
              >
                {saveMapping.isPending ? (
                  <>
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    {t("common.saving", "保存中...")}
                  </>
                ) : (
                  t("common.save", "保存")
                )}
              </Button>
            </DialogFooter>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

interface MappingItemProps {
  mapping: ProjectProviderMapping;
  providerName: string;
  providerMissing: boolean;
  disabled: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onToggleEnabled: () => void;
  isSaving: boolean;
}

function MappingItem({
  mapping,
  providerName,
  providerMissing,
  disabled,
  onEdit,
  onDelete,
  onToggleEnabled,
  isSaving,
}: MappingItemProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "rounded-lg border bg-card transition-colors overflow-hidden",
        providerMissing
          ? "border-yellow-500/50 bg-yellow-500/5"
          : mapping.enabled
            ? "border-border"
            : "border-border/50 bg-muted/30 opacity-60",
      )}
    >
      {/* 主内容区域 */}
      <div className="flex items-center gap-3 p-3">
        {/* 启用/禁用开关 */}
        <button
          onClick={onToggleEnabled}
          disabled={disabled || isSaving}
          className="flex-shrink-0 p-1 rounded-md hover:bg-muted/50 transition-colors"
          aria-label={
            mapping.enabled
              ? t("common.disable", "禁用")
              : t("common.enable", "启用")
          }
        >
          {mapping.enabled ? (
            <ToggleRight className="h-5 w-5 text-emerald-500" />
          ) : (
            <ToggleLeft className="h-5 w-5 text-muted-foreground" />
          )}
        </button>

        {/* 项目路径信息 */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <FolderTree className="h-4 w-4 text-blue-500 flex-shrink-0" />
            <span
              className="text-sm font-medium truncate"
              title={mapping.projectPath}
            >
              {mapping.displayName || mapping.projectPath}
            </span>
          </div>
          {mapping.displayName && (
            <p
              className="text-xs text-muted-foreground truncate mt-1 ml-6"
              title={mapping.projectPath}
            >
              {mapping.projectPath}
            </p>
          )}
        </div>

        {/* 操作按钮 */}
        <div className="flex items-center gap-1 flex-shrink-0">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={onEdit}
            disabled={disabled || isSaving}
            aria-label={t("common.edit", "编辑")}
          >
            <Edit2 className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground hover:text-destructive"
            onClick={onDelete}
            disabled={disabled || isSaving}
            aria-label={t("common.delete", "删除")}
          >
            {isSaving ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Trash2 className="h-4 w-4" />
            )}
          </Button>
        </div>
      </div>

      {/* 元数据栏 */}
      <div className="flex items-center gap-3 px-3 pb-3 pt-0">
        <div className="ml-10 flex items-center gap-2 flex-wrap">
          {/* 供应商标签 */}
          <div
            className={cn(
              "flex items-center gap-1.5 px-2 py-1 rounded-md",
              providerMissing
                ? "bg-yellow-500/15 text-yellow-600 dark:text-yellow-400"
                : "bg-muted/60",
            )}
          >
            {providerMissing && (
              <AlertTriangle className="h-3 w-3 flex-shrink-0" />
            )}
            <span className="text-xs text-muted-foreground">
              {t("projectMapping.provider", "供应商")}:
            </span>
            <span
              className={cn(
                "text-xs font-medium",
                providerMissing ? "text-yellow-600 dark:text-yellow-400" : "text-foreground",
              )}
            >
              {providerMissing
                ? t("projectMapping.providerMissing", "供应商不存在")
                : providerName}
            </span>
          </div>

          {/* 优先级徽章 */}
          <div
            className={cn(
              "px-2 py-1 rounded-md text-xs font-medium",
              mapping.priority <= 50
                ? "bg-orange-500/15 text-orange-600 dark:text-orange-400"
                : mapping.priority <= 100
                  ? "bg-blue-500/15 text-blue-600 dark:text-blue-400"
                  : "bg-muted/60 text-muted-foreground",
            )}
          >
            P{mapping.priority}
          </div>
        </div>
      </div>

      {/* Provider 缺失警告 */}
      {providerMissing && (
        <div className="px-3 pb-3">
          <div className="ml-10 text-xs text-yellow-600 dark:text-yellow-400 flex items-center gap-1">
            <AlertTriangle className="h-3 w-3" />
            {t(
              "projectMapping.providerMissingHint",
              "请编辑此映射并选择一个有效的供应商",
            )}
          </div>
        </div>
      )}
    </div>
  );
}
