use std::time::Duration;

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
    pub fn is_import_allowed(&self, module_name: &str) -> bool {
        // 如果在白名单中，允许
        if self
            .allowed_imports
            .iter()
            .any(|a| module_name.starts_with(a))
        {
            return true;
        }
        // 如果在黑名单中，禁止
        if self
            .blocked_imports
            .iter()
            .any(|b| module_name.starts_with(b))
        {
            return false;
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
}

impl Default for SkillSandbox {
    fn default() -> Self {
        Self::new()
    }
}
