//! 沙箱契约数据类型（与 CubeSandbox / E2B 的具体协议无关）

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 沙箱创建规格
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    /// 模板 ID 或模板别名（空 → 使用实现方的默认模板）
    pub template_id: String,
    /// 存活时长（秒），到期由服务端暂停 / 销毁
    pub timeout_secs: u32,
    /// 业务标签：本平台固定写入 `tenant` / `user_id` / `session_id` 三个键，
    /// 用于 `list` 过滤与审计
    pub metadata: BTreeMap<String, String>,
    /// 注入沙箱的环境变量（**禁止**放平台密钥，见设计文档 §8.4）
    pub env: BTreeMap<String, String>,
    /// 出网策略
    pub network: NetworkPolicy,
    /// 持久卷挂载
    pub volume_mounts: Vec<VolumeMount>,
}

/// 出网策略（缺省全禁：沙箱内不联网）
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// 禁止一切出网（缺省，安全优先）
    #[default]
    DenyAll,
    /// 仅允许访问列出的域名（由 CubeEgress 执行 L7 过滤）
    AllowDomains(Vec<String>),
}

/// 持久卷挂载
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeMount {
    /// 沙箱内挂载点
    pub path: String,
    /// 服务端返回的卷 ID
    pub volume_id: String,
    /// 是否只读挂载
    pub read_only: bool,
}

/// 沙箱句柄：后续所有操作只需要它
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SandboxHandle {
    /// 服务端沙箱 ID
    pub id: String,
    /// 当前状态
    pub state: SandboxState,
    /// 所用模板（能解析到时填充）
    pub template_id: Option<String>,
    /// 创建时间（RFC3339 字符串，原样透传）
    pub created_at: Option<String>,
    /// 数据面 Host 后缀（如 `cube.app`）
    pub data_host_suffix: String,
    /// 数据面 envd 端口
    pub envd_port: u16,
}

impl SandboxHandle {
    /// 拼出数据面虚拟主机：`{port}-{id}.{suffix}`
    #[must_use]
    pub fn data_host(&self, port: u16) -> String {
        format!("{}-{}.{}", port, self.id, self.data_host_suffix)
    }
}

/// 沙箱状态（只保留业务需要区分的四种）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub enum SandboxState {
    /// 运行中
    Running,
    /// 请求暂停、尚未完成
    Pausing,
    /// 已暂停（保留内存快照）
    Paused,
    /// `stopped` / `error` / 未知取值
    #[default]
    Unknown,
}

impl SandboxState {
    /// 从服务端状态字符串解析（大小写与首尾空白容错）
    ///
    /// 归一化规则（设计文档 §5.5）：
    /// - `running` → `Running`
    /// - `pausing` → `Pausing`
    /// - `paused` → `Paused`
    /// - 其余（`stopped` / `error` / 空串） → `Unknown`
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" => Self::Running,
            "pausing" => Self::Pausing,
            "paused" => Self::Paused,
            _ => Self::Unknown,
        }
    }

    /// 是否可执行（只有 `Running` 可直接执行，`Paused` 需先 resume）
    #[must_use]
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Running)
    }
}

/// 执行请求：`code` 与 `command` 二选一（同时给出以 `code` 为准）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecRequest {
    /// 代码片段（缺省解释器为 python3，由实现方按模板决定）
    pub code: Option<String>,
    /// shell 命令（等价于 SDK 的 `commands.run`）
    pub command: Option<String>,
    /// 命令参数（与 `command` 配合，避免自行拼 shell 字符串）
    pub argv: Vec<String>,
    /// 工作目录
    pub cwd: Option<String>,
    /// 追加环境变量
    pub env: BTreeMap<String, String>,
    /// 执行身份，缺省 `root`
    pub user: Option<String>,
    /// 单次执行超时（秒），缺省取实现方配置
    pub timeout_secs: Option<u32>,
}

impl ExecRequest {
    /// 用代码构造执行请求
    #[must_use]
    pub fn code(code: impl Into<String>) -> Self {
        Self {
            code: Some(code.into()),
            ..Self::default()
        }
    }

    /// 用命令构造执行请求
    #[must_use]
    pub fn command(command: impl Into<String>) -> Self {
        Self {
            command: Some(command.into()),
            ..Self::default()
        }
    }

    /// 是否为代码执行
    #[must_use]
    pub fn is_code(&self) -> bool {
        self.code.as_ref().is_some_and(|c| !c.trim().is_empty())
    }
}

/// 执行结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExecResult {
    /// 进程退出码（0 表示正常退出）
    pub exit_code: i32,
    /// 主输出（对应 SDK 的 `result.text`）
    pub text: Option<String>,
    /// 逐行 stdout
    pub stdout: Vec<String>,
    /// 逐行 stderr
    pub stderr: Vec<String>,
    /// 执行异常（Python traceback 等）
    pub error: Option<ExecFailure>,
    /// 耗时（毫秒）
    pub duration_ms: u64,
}

impl ExecResult {
    /// 是否成功（退出码 0 且无执行异常）
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.exit_code == 0 && self.error.is_none()
    }

    /// 把 stdout / stderr 拼成可直接展示的文本（stdout 优先，其次 stderr）
    #[must_use]
    pub fn combined_output(&self) -> String {
        if let Some(text) = self.text.as_ref().filter(|t| !t.trim().is_empty()) {
            return text.clone();
        }
        if !self.stdout.is_empty() {
            return self.stdout.join("\n");
        }
        self.stderr.join("\n")
    }
}

/// 执行异常详情
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExecFailure {
    /// 异常类型，如 `ZeroDivisionError`
    pub name: String,
    /// 异常信息
    pub value: String,
    /// 堆栈
    pub traceback: Vec<String>,
}

/// 沙箱内文件条目
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FileEntry {
    /// 绝对路径
    pub path: String,
    /// 文件名
    pub name: String,
    /// 是否目录
    pub is_dir: bool,
    /// 字节数（目录为 0）
    pub size: u64,
}

/// 列表项
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSummary {
    /// 沙箱 ID
    pub id: String,
    /// 状态
    pub state: SandboxState,
    /// 模板
    pub template_id: Option<String>,
    /// 业务标签
    pub metadata: BTreeMap<String, String>,
    /// 启动时间（RFC3339 字符串，原样透传）
    pub started_at: Option<String>,
}

/// 列表过滤条件（`metadata` 全量匹配）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxFilter {
    /// 业务标签全量匹配
    pub metadata: BTreeMap<String, String>,
    /// 状态过滤（空 → 不限）
    pub states: Vec<SandboxState>,
    /// 条数上限（缺省 100，最大 [`crate::MAX_LIST_LIMIT`]）
    pub limit: Option<u32>,
}

/// 健康检查结果
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct HealthInfo {
    /// 控制面是否可用
    pub ok: bool,
    /// 服务端版本（能解析到时填充）
    pub version: Option<String>,
    /// 附加信息（未启用 / 探测失败原因）
    pub message: Option<String>,
}

/// 快照信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// 快照名
    pub name: String,
    /// 所属沙箱
    pub sandbox_id: String,
    /// 创建时间（RFC3339 字符串）
    pub created_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle(suffix: &str) -> SandboxHandle {
        SandboxHandle {
            id: "sb-1".to_string(),
            state: SandboxState::Running,
            template_id: None,
            created_at: None,
            data_host_suffix: suffix.to_string(),
            envd_port: crate::DEFAULT_ENVD_PORT,
        }
    }

    #[test]
    fn data_host_joins_port_id_and_suffix() {
        assert_eq!(handle("cube.app").data_host(49983), "49983-sb-1.cube.app");
        assert_eq!(handle("cube.app").data_host(49999), "49999-sb-1.cube.app");
    }

    #[test]
    fn exec_result_is_ok_requires_zero_exit_and_no_error() {
        let ok = ExecResult {
            exit_code: 0,
            text: Some("2".to_string()),
            stdout: vec![],
            stderr: vec![],
            error: None,
            duration_ms: 1,
        };
        assert!(ok.is_ok());
        assert_eq!(ok.combined_output(), "2");

        let failed_exit = ExecResult {
            exit_code: 1,
            ..ok.clone()
        };
        assert!(!failed_exit.is_ok());

        let with_traceback = ExecResult {
            error: Some(ExecFailure {
                name: "ZeroDivisionError".to_string(),
                value: "division by zero".to_string(),
                traceback: vec!["line 1".to_string()],
            }),
            ..ok
        };
        assert!(!with_traceback.is_ok());
    }

    #[test]
    fn combined_output_falls_back_to_stdout_then_stderr() {
        let mut result = ExecResult {
            exit_code: 0,
            text: Some("   ".to_string()),
            stdout: vec!["a".to_string(), "b".to_string()],
            stderr: vec!["warn".to_string()],
            error: None,
            duration_ms: 0,
        };
        assert_eq!(result.combined_output(), "a\nb");
        result.stdout.clear();
        assert_eq!(result.combined_output(), "warn");
    }

    #[test]
    fn network_policy_defaults_to_deny_all() {
        assert_eq!(NetworkPolicy::default(), NetworkPolicy::DenyAll);
    }

    #[test]
    fn state_parsing_covers_known_and_unknown_values() {
        assert_eq!(SandboxState::parse("running"), SandboxState::Running);
        assert_eq!(SandboxState::parse("  RUNNING "), SandboxState::Running);
        assert_eq!(SandboxState::parse("pausing"), SandboxState::Pausing);
        assert_eq!(SandboxState::parse("paused"), SandboxState::Paused);
        assert_eq!(SandboxState::parse("stopped"), SandboxState::Unknown);
        assert_eq!(SandboxState::parse("error"), SandboxState::Unknown);
        assert_eq!(SandboxState::parse(""), SandboxState::Unknown);
        assert!(SandboxState::Running.is_usable());
        assert!(!SandboxState::Paused.is_usable());
    }

    #[test]
    fn exec_request_builders_and_code_detection() {
        assert!(ExecRequest::code("print(1)").is_code());
        assert!(!ExecRequest::code("   ").is_code());
        assert!(!ExecRequest::command("ls").is_code());
    }
}
