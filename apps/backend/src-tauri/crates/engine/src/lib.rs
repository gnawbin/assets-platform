pub mod python_runtime;
pub mod skill_context;
pub mod skill_registry;
pub mod skill_sandbox;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Zen Engine 调度器
pub struct ZenEngine {
    task_queue: mpsc::Sender<WorkflowTask>,
    running_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    skill_registry: Arc<skill_registry::SkillRegistry>,
}

/// 工作流任务
#[derive(Debug, Clone)]
pub struct WorkflowTask {
    pub id: String,
    pub skill_id: String,
    pub priority: u8, // 0=最高, 255=最低
    pub params: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ZenEngine {
    /// 初始化 Zen Engine
    pub async fn start() -> Result<Self, String> {
        let (tx, mut rx) = mpsc::channel::<WorkflowTask>(1000);

        let running_tasks = Arc::new(Mutex::new(HashMap::new()));
        let running_tasks_clone = running_tasks.clone();

        // 后台任务处理循环
        tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                let task_id = task.id.clone();
                let handle = tokio::spawn(async move {
                    tracing::info!("执行任务: {} (skill: {})", task_id, task.skill_id);
                    // 实际执行逻辑由 skill_registry 处理
                });
                let mut tasks = running_tasks_clone.lock().await;
                tasks.insert(task.id.clone(), handle);
            }
        });

        // 初始化 Skill 注册表（暂不加载 Python，仅创建空注册表）
        let skill_registry = Arc::new(skill_registry::SkillRegistry::new());

        Ok(ZenEngine {
            task_queue: tx,
            running_tasks,
            skill_registry,
        })
    }

    /// 提交任务
    pub async fn submit_task(&self, task: WorkflowTask) -> Result<(), String> {
        self.task_queue
            .send(task)
            .await
            .map_err(|e| format!("提交任务失败: {}", e))
    }

    /// 获取 Skill 注册表引用
    pub fn skill_registry(&self) -> Arc<skill_registry::SkillRegistry> {
        self.skill_registry.clone()
    }
}
