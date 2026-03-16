//! 请求上下文模块
//!
//! 提供请求生命周期的上下文管理，封装通用初始化逻辑

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::{
    extract_session_id,
    forwarder::RequestForwarder,
    server::ProxyState,
    types::{AppProxyConfig, OptimizerConfig, RectifierConfig},
    ProxyError,
};
use crate::services::SessionCache;
use axum::http::HeaderMap;
use std::time::Instant;

/// 流式超时配置
#[derive(Debug, Clone, Copy)]
pub struct StreamingTimeoutConfig {
    /// 首字节超时（秒），0 表示禁用
    pub first_byte_timeout: u64,
    /// 静默期超时（秒），0 表示禁用
    pub idle_timeout: u64,
}

/// 请求上下文
///
/// 贯穿整个请求生命周期，包含：
/// - 计时信息
/// - 应用级代理配置（per-app）
/// - 选中的 Provider 列表（用于故障转移）
/// - 请求模型名称
/// - 日志标签
/// - Session ID（用于日志关联）
pub struct RequestContext {
    /// 请求开始时间
    pub start_time: Instant,
    /// 应用级代理配置（per-app，包含重试次数和超时配置）
    pub app_config: AppProxyConfig,
    /// 选中的 Provider（故障转移链的第一个）
    pub provider: Provider,
    /// 完整的 Provider 列表（用于故障转移）
    providers: Vec<Provider>,
    /// 请求开始时的"当前供应商"（用于判断是否需要同步 UI/托盘）
    ///
    /// 这里使用本地 settings 的设备级 current provider。
    /// 代理模式下如果实际使用的 provider 与此不一致，会触发切换以确保 UI 始终准确。
    pub current_provider_id: String,
    /// 请求中的模型名称
    pub request_model: String,
    /// 日志标签（如 "Claude"、"Codex"、"Gemini"）
    pub tag: &'static str,
    /// 应用类型字符串（如 "claude"、"codex"、"gemini"）
    pub app_type_str: &'static str,
    /// 应用类型（预留，目前通过 app_type_str 使用）
    #[allow(dead_code)]
    pub app_type: AppType,
    /// Session ID（从客户端请求提取或新生成）
    pub session_id: String,
    /// 整流器配置
    pub rectifier_config: RectifierConfig,
    /// 优化器配置
    pub optimizer_config: OptimizerConfig,
    /// 当前 Provider 是否是通过项目目录映射匹配到的
    ///
    /// 如果为 true，请求成功后不应该触发全局供应商切换
    pub is_project_mapped_provider: bool,
}

impl RequestContext {
    /// 创建请求上下文
    ///
    /// # Arguments
    /// * `state` - 代理服务器状态
    /// * `body` - 请求体 JSON
    /// * `headers` - 请求头（用于提取 Session ID）
    /// * `app_type` - 应用类型
    /// * `tag` - 日志标签
    /// * `app_type_str` - 应用类型字符串
    ///
    /// # Errors
    /// 返回 `ProxyError` 如果 Provider 选择失败
    pub async fn new(
        state: &ProxyState,
        body: &serde_json::Value,
        headers: &HeaderMap,
        app_type: AppType,
        tag: &'static str,
        app_type_str: &'static str,
    ) -> Result<Self, ProxyError> {
        let start_time = Instant::now();

        // 从数据库读取应用级代理配置（per-app）
        let app_config = state
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 从数据库读取整流器配置
        let rectifier_config = state.db.get_rectifier_config().unwrap_or_default();
        let optimizer_config = state.db.get_optimizer_config().unwrap_or_default();

        let current_provider_id =
            crate::settings::get_current_provider(&app_type).unwrap_or_default();

        // 从请求体提取模型名称
        let request_model = body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 提取 Session ID
        let session_result = extract_session_id(headers, body, app_type_str);
        let session_id = session_result.session_id.clone();

        log::info!(
            "[{}] Session ID: {} (from {:?}, client_provided: {})",
            tag,
            session_id,
            session_result.source,
            session_result.client_provided
        );

        // 使用共享的 ProviderRouter 选择 Provider（熔断器状态跨请求保持）
        // 注意：只在这里调用一次，结果传递给 forwarder，避免重复消耗 HalfOpen 名额
        let providers = state
            .provider_router
            .select_providers(app_type_str)
            .await
            .map_err(|e| match e {
                crate::error::AppError::AllProvidersCircuitOpen => {
                    ProxyError::AllProvidersCircuitOpen
                }
                crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
                _ => ProxyError::DatabaseError(e.to_string()),
            })?;

        // 尝试根据 session_id 匹配项目目录映射
        let (provider, providers, is_project_mapped_provider) = Self::try_match_project_provider(
            &state,
            &session_id,
            app_type_str,
            providers,
        ).await;

        let provider = provider
            .ok_or(ProxyError::NoAvailableProvider)?;

        log::info!(
            "[{}] 使用 Provider: {} (model: {}, project_mapped: {}, failover_chain: {})",
            tag,
            provider.name,
            request_model,
            is_project_mapped_provider,
            providers.len()
        );

        Ok(Self {
            start_time,
            app_config,
            provider,
            providers,
            current_provider_id,
            request_model,
            tag,
            app_type_str,
            app_type,
            session_id,
            rectifier_config,
            optimizer_config,
            is_project_mapped_provider,
        })
    }

    /// 从 URI 提取模型名称（Gemini 专用）
    ///
    /// Gemini API 的模型名称在 URI 中，格式如：
    /// `/v1beta/models/gemini-pro:generateContent`
    pub fn with_model_from_uri(mut self, uri: &axum::http::Uri) -> Self {
        let endpoint = uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or(uri.path());

        self.request_model = endpoint
            .split('/')
            .find(|s| s.starts_with("models/"))
            .and_then(|s| s.strip_prefix("models/"))
            .map(|s| s.split(':').next().unwrap_or(s))
            .unwrap_or("unknown")
            .to_string();

        self
    }

    /// 创建 RequestForwarder
    ///
    /// 使用共享的 ProviderRouter，确保熔断器状态跨请求保持
    ///
    /// 配置生效规则：
    /// - 故障转移开启：超时配置正常生效（0 表示禁用超时）
    /// - 故障转移关闭：超时配置不生效（全部传入 0）
    pub fn create_forwarder(&self, state: &ProxyState) -> RequestForwarder {
        let (non_streaming_timeout, first_byte_timeout, idle_timeout) =
            if self.app_config.auto_failover_enabled {
                // 故障转移开启：使用配置的值（0 = 禁用超时）
                (
                    self.app_config.non_streaming_timeout as u64,
                    self.app_config.streaming_first_byte_timeout as u64,
                    self.app_config.streaming_idle_timeout as u64,
                )
            } else {
                // 故障转移关闭：不启用超时配置
                log::debug!(
                    "[{}] Failover disabled, timeout configs are bypassed",
                    self.tag
                );
                (0, 0, 0)
            };

        RequestForwarder::new(
            state.provider_router.clone(),
            non_streaming_timeout,
            state.status.clone(),
            state.current_providers.clone(),
            state.failover_manager.clone(),
            state.app_handle.clone(),
            self.current_provider_id.clone(),
            first_byte_timeout,
            idle_timeout,
            self.rectifier_config.clone(),
            self.optimizer_config.clone(),
            self.is_project_mapped_provider,
        )
    }

    /// 获取 Provider 列表（用于故障转移）
    ///
    /// 返回在创建上下文时已选择的 providers，避免重复调用 select_providers()
    pub fn get_providers(&self) -> Vec<Provider> {
        self.providers.clone()
    }

    /// 计算请求延迟（毫秒）
    #[inline]
    pub fn latency_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// 获取流式超时配置
    ///
    /// 配置生效规则：
    /// - 故障转移开启：返回配置的值（0 表示禁用超时检查）
    /// - 故障转移关闭：返回 0（禁用超时检查）
    #[inline]
    pub fn streaming_timeout_config(&self) -> StreamingTimeoutConfig {
        if self.app_config.auto_failover_enabled {
            // 故障转移开启：使用配置的值（0 = 禁用超时）
            StreamingTimeoutConfig {
                first_byte_timeout: self.app_config.streaming_first_byte_timeout as u64,
                idle_timeout: self.app_config.streaming_idle_timeout as u64,
            }
        } else {
            // 故障转移关闭：禁用流式超时检查
            StreamingTimeoutConfig {
                first_byte_timeout: 0,
                idle_timeout: 0,
            }
        }
    }

    /// 尝试根据 session_id 匹配项目目录映射
    ///
    /// 如果匹配成功，返回匹配的 Provider、重新排序的 Provider 列表和 true；
    /// 否则返回原始 Provider 列表的第一个和 false。
    async fn try_match_project_provider(
        state: &ProxyState,
        session_id: &str,
        app_type_str: &str,
        mut providers: Vec<Provider>,
    ) -> (Option<Provider>, Vec<Provider>, bool) {
        // 1. 从 SessionCache 获取 cwd
        let session_cache = SessionCache::instance();
        let cwd = session_cache.get_cwd(session_id).await;

        let Some(cwd) = cwd else {
            return (providers.first().cloned(), providers, false);
        };

        // 2. 匹配项目目录映射
        let mapping = state.db.match_project_mapping(&cwd, app_type_str).ok().flatten();

        let Some(mapping) = mapping else {
            return (providers.first().cloned(), providers, false);
        };

        // 3. 先在已有列表中查找（避免额外数据库查询）
        let matched_provider = providers.iter().position(|p| p.id == mapping.provider_id);

        if let Some(idx) = matched_provider {
            // 将匹配的 Provider 移到列表首位
            let provider = providers.remove(idx);
            providers.insert(0, provider.clone());
            log::info!(
                "[{}] 项目映射命中: {} -> {}",
                app_type_str,
                cwd,
                provider.name
            );
            return (Some(provider), providers, true);
        }

        // 4. 不在列表中，从数据库获取所有 Provider 查找
        let all_providers = state.db.get_all_providers(app_type_str);
        if let Ok(all_providers) = all_providers {
            if let Some(provider) = all_providers.get(&mapping.provider_id).cloned() {
                log::info!(
                    "[{}] 项目映射命中: {} -> {}",
                    app_type_str,
                    cwd,
                    provider.name
                );
                // 将匹配的 Provider 插入到列表首位
                providers.insert(0, provider.clone());
                return (Some(provider), providers, true);
            } else {
                log::warn!(
                    "[{}] 项目映射的 Provider {} 不存在",
                    app_type_str,
                    mapping.provider_id
                );
            }
        }

        log::warn!(
            "[{}] 项目映射的 Provider {} 不存在",
            app_type_str,
            mapping.provider_id
        );
        (providers.first().cloned(), providers, false)
    }
}
