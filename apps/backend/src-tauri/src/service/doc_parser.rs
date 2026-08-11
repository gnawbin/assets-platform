//! Python doc-parser 侧车客户端
//!
//! 调用 doc-parser 服务将非文本文件（PDF/Word/图片/音频/视频）解析为纯文本 + 图片路径。
//! 服务地址由环境变量 `DOC_PARSER_URL` 控制（默认 `http://127.0.0.1:8321`），
//! 认证令牌 `DOC_PARSER_TOKEN` 由 Tauri 启动时注入。
//!
//! 相关文档：docs/知识库模块/智能问答多模态（文件与视频上传）设计方案.md

use serde::{Deserialize, Serialize};
use tracing::{error, info};

/// 解析结果（与 doc-parser 的 ParseResult 对齐）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    /// 原文件名
    pub file_name: String,
    /// 文件类型: pdf / document / image / audio / video
    pub file_type: String,
    /// 提取或生成的纯文本内容
    pub raw_text: String,
    /// 需要 VLM 语义描述的本地图片路径（图片原图 / 视频抽帧）
    #[serde(default)]
    pub images: Vec<String>,
    /// 可选元数据
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Python 侧车 HTTP 客户端
pub struct DocParserClient {
    base_url: String,
    token: String,
    http_client: reqwest::Client,
}

impl Default for DocParserClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DocParserClient {
    pub fn new() -> Self {
        let base_url = std::env::var("DOC_PARSER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8321".to_string());
        let token = std::env::var("DOC_PARSER_TOKEN").unwrap_or_default();

        let http_client = reqwest::Client::builder()
            // 视频/音频解析耗时长，放宽超时（10 分钟）
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .unwrap_or_else(|e| {
                error!("构建 doc-parser HTTP 客户端失败: {}", e);
                reqwest::Client::new()
            });

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            http_client,
        }
    }

    /// 解析本地文件 → 纯文本 + 图片路径
    ///
    /// `options` 常见字段：
    /// - `skip_index`: 视频解析后是否跳过向量化入库（聊天场景必须 `true`，仅解析）
    /// - `frame_interval`: 视频抽帧间隔（秒）
    pub async fn parse_file(
        &self,
        file_path: &str,
        options: &serde_json::Value,
    ) -> Result<ParseResult, String> {
        let url = format!("{}/parse", self.base_url);
        let body = serde_json::json!({
            "file_path": file_path,
            "options": options,
        });

        let mut req = self.http_client.post(&url).json(&body);
        if !self.token.is_empty() {
            req = req.header("X-API-Token", &self.token);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("doc-parser 请求失败: {}", e))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("doc-parser 读取响应失败: {}", e))?;

        if !status.is_success() {
            return Err(format!("doc-parser 解析失败 [{}]: {}", status, text));
        }

        match serde_json::from_str::<ParseResult>(&text) {
            Ok(result) => {
                info!(
                    "doc-parser 解析成功: file={}, type={}, text_len={}, images={}",
                    result.file_name,
                    result.file_type,
                    result.raw_text.len(),
                    result.images.len()
                );
                Ok(result)
            }
            Err(e) => Err(format!("doc-parser 响应解析失败: {}", e)),
        }
    }
}
