//! 厂商预设表
//!
//! 新增一家 OpenAI 兼容厂商时**只改这张表**，不需要动路由与工厂逻辑：
//! 协议、默认 base_url（含版本段）、默认模型都在这里声明。
//!
//! 火山方舟 `/api/v3`、GLM `/api/paas/v4` 这类非 `/v1` 路径同样由预设表消化，
//! 避免旧实现「硬拼 /v1」导致的 404。

use crate::provider::Protocol;

/// 单个厂商预设
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    /// 与 `llm_provider.provider_code` 对应
    pub provider_code: &'static str,
    /// 线上协议族
    pub protocol: Protocol,
    /// 默认 base_url（已含版本段，供工厂兜底）
    pub base_url: &'static str,
    /// 数据库中未配置模型时使用的默认模型
    pub default_model: &'static str,
}

/// 厂商预设表
///
/// `openai` / `qwen` / `deepseek` / `volcengine` / `tencent` / `ollama` / `glm` /
/// `moonshot` / `openrouter` / `groq` / `xai` / `siliconflow` 走 OpenAI Chat
/// Completions 协议；`claude` 走 Anthropic Messages 协议。
pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        provider_code: "openai",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o",
    },
    ProviderPreset {
        provider_code: "claude",
        protocol: Protocol::AnthropicMessages,
        base_url: "https://api.anthropic.com",
        default_model: "claude-sonnet-4-20250514",
    },
    ProviderPreset {
        provider_code: "qwen",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-plus",
    },
    ProviderPreset {
        provider_code: "deepseek",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
    },
    ProviderPreset {
        provider_code: "volcengine",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        default_model: "doubao-pro-32k",
    },
    ProviderPreset {
        provider_code: "tencent",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        default_model: "hunyuan-lite",
    },
    ProviderPreset {
        provider_code: "ollama",
        protocol: Protocol::OpenAiCompatible,
        base_url: "http://localhost:11434/v1",
        default_model: "llama3.2",
    },
    ProviderPreset {
        provider_code: "glm",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        default_model: "glm-4-plus",
    },
    ProviderPreset {
        provider_code: "moonshot",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.moonshot.cn/v1",
        default_model: "moonshot-v1-8k",
    },
    ProviderPreset {
        provider_code: "openrouter",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-4o",
    },
    ProviderPreset {
        provider_code: "groq",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
    },
    ProviderPreset {
        provider_code: "xai",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.x.ai/v1",
        default_model: "grok-2-latest",
    },
    ProviderPreset {
        provider_code: "siliconflow",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.siliconflow.cn/v1",
        default_model: "Qwen/Qwen2.5-72B-Instruct",
    },
];

/// 按厂商编码查找预设（大小写不敏感、忽略首尾空白）
pub fn lookup_preset(provider_code: &str) -> Option<&'static ProviderPreset> {
    let code = provider_code.trim();
    PRESETS
        .iter()
        .find(|preset| preset.provider_code.eq_ignore_ascii_case(code))
}

/// 默认 base_url（未知厂商返回 `None`）
pub fn default_base_url(provider_code: &str) -> Option<&'static str> {
    lookup_preset(provider_code).map(|preset| preset.base_url)
}

/// 默认模型（未知厂商返回 `None`）
pub fn default_model(provider_code: &str) -> Option<&'static str> {
    lookup_preset(provider_code).map(|preset| preset.default_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive_and_trims() {
        assert_eq!(
            lookup_preset(" OpenAI ").map(|p| p.provider_code),
            Some("openai")
        );
        assert_eq!(
            lookup_preset("CLAUDE").map(|p| p.protocol),
            Some(Protocol::AnthropicMessages)
        );
        assert!(lookup_preset("no-such-vendor").is_none());
    }

    #[test]
    fn claude_uses_anthropic_protocol_and_others_are_openai_compatible() {
        assert_eq!(
            lookup_preset("claude").unwrap().protocol,
            Protocol::AnthropicMessages
        );
        for code in [
            "openai",
            "qwen",
            "deepseek",
            "volcengine",
            "tencent",
            "ollama",
            "glm",
            "moonshot",
            "openrouter",
            "groq",
            "xai",
            "siliconflow",
        ] {
            let preset = lookup_preset(code).unwrap_or_else(|| panic!("缺少预设: {code}"));
            assert_eq!(preset.protocol, Protocol::OpenAiCompatible, "{code}");
        }
    }

    #[test]
    fn every_preset_has_versioned_base_url_and_default_model() {
        for preset in PRESETS {
            assert!(!preset.default_model.is_empty(), "{}", preset.provider_code);
            assert!(
                preset.base_url.starts_with("http"),
                "{} 的 base_url 非法",
                preset.provider_code
            );
            // 版本段必须写在 base_url 里，实现侧不再硬拼 /v1
            assert!(
                !preset.base_url.ends_with('/'),
                "{} 的 base_url 不应以 / 结尾",
                preset.provider_code
            );
        }
        assert_eq!(
            default_base_url("deepseek"),
            Some("https://api.deepseek.com/v1")
        );
        assert_eq!(default_model("qwen"), Some("qwen-plus"));
    }
}

