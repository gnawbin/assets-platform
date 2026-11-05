//! 客户端配置
//!
//! 密钥只在此结构中持有，`Debug` 输出一律脱敏，避免日志泄漏。

use std::time::Duration;

/// 默认超时：模型生成长文本可能较慢，留足余量
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// 默认传输层重试次数
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// 协议实现共用的客户端配置
#[derive(Clone)]
pub struct LlmClientConfig {
    /// 已归一化的 base_url（含版本段，如 `https://api.openai.com/v1`）
    pub base_url: String,
    /// API Key（明文只存在于内存）
    pub api_key: String,
    /// 未指定模型时使用的默认模型
    pub default_model: String,
    /// 追加的请求头（如 Azure 的 `api-key`、OpenRouter 的 `HTTP-Referer`）
    pub headers: Vec<(String, String)>,
    /// 单次请求超时
    pub timeout: Duration,
    /// 传输层重试次数（仅对可重试错误生效）
    pub max_retries: u32,
}

impl LlmClientConfig {
    /// 新建配置
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            default_model: default_model.into(),
            headers: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// 追加一个请求头
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// 覆盖超时
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 覆盖重试次数
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// 密钥是否为空（本地 Ollama 等场景允许空密钥）
    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// 脱敏后的密钥，可直接写日志
    pub fn masked_api_key(&self) -> String {
        let key = self.api_key.trim();
        if key.is_empty() {
            return "<empty>".to_string();
        }
        if key.len() <= 8 {
            return "****".to_string();
        }
        format!("{}****{}", &key[..4], &key[key.len() - 4..])
    }
}

impl std::fmt::Debug for LlmClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmClientConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.masked_api_key())
            .field("default_model", &self.default_model)
            .field("headers", &self.headers)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_masks_api_key() {
        let config = LlmClientConfig::new("https://api.openai.com/v1", "sk-abcdef123456", "gpt-4o");
        let printed = format!("{config:?}");
        assert!(!printed.contains("sk-abcdef123456"), "{printed}");
        assert!(printed.contains("sk-a****3456"), "{printed}");
    }

    #[test]
    fn masked_api_key_handles_short_and_empty() {
        let short = LlmClientConfig::new("http://localhost", "abc", "llama3.2");
        assert_eq!(short.masked_api_key(), "****");
        assert!(short.has_api_key());

        let empty = LlmClientConfig::new("http://localhost", "  ", "llama3.2");
        assert_eq!(empty.masked_api_key(), "<empty>");
        assert!(!empty.has_api_key());
    }
}
