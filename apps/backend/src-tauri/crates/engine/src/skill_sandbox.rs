use std::time::Duration;

use sandbox::{ExecRequest, MAX_CODE_BYTES};

/// Skill 执行沙箱（安全隔离）
pub struct SkillSandbox {
    max_execution_time: Duration,
    max_output_size: usize,
    allowed_imports: Vec<String>,
    blocked_imports: Vec<String>,
}

impl SkillSandbox {
    pub fn new() -> Self {
        SkillSandbox {
            max_execution_time: Duration::from_secs(60),
            max_output_size: 1024 * 1024,
            allowed_imports: vec![
                "zen_engine".into(),
                "json".into(),
                "re".into(),
                "typing".into(),
                "math".into(),
                "datetime".into(),
                "collections".into(),
                "itertools".into(),
            ],
            blocked_imports: vec![
                "os".into(),
                "subprocess".into(),
                "shutil".into(),
                "socket".into(),
                "requests".into(),
                "http".into(),
                "urllib".into(),
                "pathlib".into(),
                "sys".into(),
                "importlib".into(),
                "ctypes".into(),
                "multiprocessing".into(),
                "threading".into(),
            ],
        }
    }

    /// 检查导入是否被允许
    ///
    /// 判定顺序（安全优先）：**黑名单优先**，再查白名单，最后缺省允许。
    /// 反例：若先查白名单，`requests` 会因匹配到白名单前缀 `re` 而被放行。
    pub fn is_import_allowed(&self, module_name: &str) -> bool {
        // 如果在黑名单中，禁止
        if self
            .blocked_imports
            .iter()
            .any(|b| module_name.starts_with(b))
        {
            return false;
        }
        // 如果在白名单中，允许
        if self
            .allowed_imports
            .iter()
            .any(|a| module_name.starts_with(a))
        {
            return true;
        }
        // 默认允许（后续可收紧）
        true
    }

    /// 验证输出大小
    pub fn validate_output_size(&self, output: &str) -> Result<(), String> {
        if output.len() > self.max_output_size {
            return Err(format!(
                "Skill 输出超过大小限制: {} > {}",
                output.len(),
                self.max_output_size
            ));
        }
        Ok(())
    }

    /// 获取最大执行时间
    pub fn max_execution_time(&self) -> Duration {
        self.max_execution_time
    }

    /// 设置最大执行时间
    pub fn set_max_execution_time(&mut self, duration: Duration) {
        self.max_execution_time = duration;
    }

    /// 设置最大输出大小
    pub fn set_max_output_size(&mut self, size: usize) {
        self.max_output_size = size;
    }

    /// 执行前预检：代码长度上限 + `import` 白名单
    ///
    /// 真正的隔离由远端沙箱（KVM MicroVM）提供，这里只做**快速失败**，
    /// 避免把明显越权的脚本送进沙箱再等着报错。
    ///
    /// # 错误
    /// 返回可直接展示给用户的中文提示（超长 / 导入了被禁模块）。
    pub fn precheck(&self, code: &str) -> Result<(), String> {
        if code.len() > MAX_CODE_BYTES {
            return Err(format!("Skill 代码超过上限 {MAX_CODE_BYTES} 字节"));
        }

        for line in code.lines() {
            let trimmed = line.trim_start();
            let module = if let Some(rest) = trimmed.strip_prefix("import ") {
                rest.split(['.', ' ', ',']).next().unwrap_or_default()
            } else if let Some(rest) = trimmed.strip_prefix("from ") {
                rest.split([' ', '.']).next().unwrap_or_default()
            } else {
                continue;
            };

            let module = module.trim();
            if module.is_empty() || module.starts_with('#') {
                continue;
            }
            if !self.is_import_allowed(module) {
                return Err(format!("Skill 代码禁止导入模块：{module}"));
            }
        }

        Ok(())
    }

    /// 构造沙箱执行请求（把本沙箱的时限/输出约束翻译成契约请求）
    ///
    /// `engine` 只依赖 `crates/sandbox` 的契约，不感知 CubeSandbox 实现。
    #[must_use]
    pub fn build_exec_request(&self, code: String) -> ExecRequest {
        let timeout_secs = u32::try_from(self.max_execution_time.as_secs()).unwrap_or(u32::MAX);
        ExecRequest {
            timeout_secs: Some(timeout_secs.max(1)),
            ..ExecRequest::code(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precheck_allows_whitelisted_imports() {
        let sandbox = SkillSandbox::new();
        assert!(sandbox.precheck("import json\nprint(json.dumps({}))").is_ok());
        assert!(sandbox.precheck("from datetime import datetime").is_ok());
        assert!(sandbox.precheck("x = 1 + 1").is_ok());
    }

    #[test]
    fn precheck_rejects_blocked_imports() {
        let sandbox = SkillSandbox::new();
        let error = sandbox
            .precheck("import os\nos.system('rm -rf /')")
            .expect_err("os 必须被拒绝");
        assert!(error.contains("os"), "{error}");

        let error = sandbox
            .precheck("from subprocess import run")
            .expect_err("subprocess 必须被拒绝");
        assert!(error.contains("subprocess"), "{error}");
    }

    #[test]
    fn blocked_modules_win_over_whitelist_prefixes() {
        let sandbox = SkillSandbox::new();
        // `requests` 以白名单前缀 `re` 开头，必须先命中黑名单被拒绝
        assert!(!sandbox.is_import_allowed("requests"));
        assert!(!sandbox.is_import_allowed("pathlib"));
        assert!(sandbox.is_import_allowed("re"));
        assert!(sandbox.is_import_allowed("json"));
        assert!(sandbox.is_import_allowed("numpy"));
    }

    #[test]
    fn precheck_rejects_oversized_code() {
        let sandbox = SkillSandbox::new();
        let code = "x".repeat(MAX_CODE_BYTES + 1);
        assert!(sandbox.precheck(&code).is_err());
    }

    #[test]
    fn build_exec_request_carries_timeout_and_code() {
        let mut sandbox = SkillSandbox::new();
        sandbox.set_max_execution_time(Duration::from_secs(45));
        let request = sandbox.build_exec_request("print(1)".to_string());
        assert_eq!(request.timeout_secs, Some(45));
        assert!(request.is_code());
        assert_eq!(request.code.as_deref(), Some("print(1)"));
    }
}

impl Default for SkillSandbox {
    fn default() -> Self {
        Self::new()
    }
}
