//! 多租户工作流运行时与业务服务。
//!
//! 每个租户 schema 拥有独立的 bpmninja 引擎、PostgreSQL 持久化、Agent
//! checkpointer 与 Worker。Tauri 命令和 HTTP 路由共享同一个 `WorkflowManager`。

use crate::agent::{AgentConfig, AgentGraphRunner, AgentInvocation, AgentOutcome};
use crate::persistence::{PostgresCheckpointer, PostgresWorkflowPersistence};
use crate::projection::{append_node_result, sync_execution};
use crate::types::*;
use chrono::Utc;
use engine_core::WorkflowEngine;
use engine_core::persistence::WorkflowPersistence;
use serde_json::{json, Value};
use service::llm_gateway_service::LLMRouter;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// 单个租户的运行时。
pub struct TenantWorkflowRuntime {
    pub schema: String,
    pub engine: Arc<WorkflowEngine>,
    pub checkpoints: Arc<PostgresCheckpointer>,
    pub runner: Arc<AgentGraphRunner>,
    /// ServiceTask Worker 的常驻任务句柄；持有它即可在运行时存活期间持续拉取任务。
    #[allow(dead_code)]
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// 全局工作流管理器。
pub struct WorkflowManager {
    llm: Arc<LLMRouter>,
    runtimes: Mutex<HashMap<String, Arc<TenantWorkflowRuntime>>>,
}

static GLOBAL: OnceLock<Arc<WorkflowManager>> = OnceLock::new();

impl WorkflowManager {
    /// 初始化全局管理器。重复调用返回已有实例。
    pub fn init(llm: Arc<LLMRouter>) -> Arc<Self> {
        GLOBAL
            .get_or_init(|| {
                Arc::new(Self {
                    llm,
                    runtimes: Mutex::new(HashMap::new()),
                })
            })
            .clone()
    }

    /// 获取全局管理器。
    pub fn global() -> Result<Arc<Self>, String> {
        GLOBAL
            .get()
            .cloned()
            .ok_or_else(|| "工作流运行时尚未初始化".to_string())
    }

    /// 获取或创建指定 schema 的运行时。
    pub async fn runtime(&self, schema: &str) -> Result<Arc<TenantWorkflowRuntime>, String> {
        {
            let runtimes = self.runtimes.lock().await;
            if let Some(runtime) = runtimes.get(schema) {
                return Ok(runtime.clone());
            }
        }

        let runtime = self.create_runtime(schema.to_string()).await?;
        let mut runtimes = self.runtimes.lock().await;
        Ok(runtimes
            .entry(schema.to_string())
            .or_insert_with(|| runtime.clone())
            .clone())
    }

    async fn create_runtime(&self, schema: String) -> Result<Arc<TenantWorkflowRuntime>, String> {
        let persistence = Arc::new(PostgresWorkflowPersistence::new(schema.clone()));
        let checkpoints = Arc::new(PostgresCheckpointer::new(schema.clone()));
        let engine = Arc::new(
            WorkflowEngine::new()
                .with_persistence(persistence.clone() as Arc<dyn WorkflowPersistence>),
        );

        database::with_schema(schema.clone(), async {
            restore_engine(&engine, persistence.as_ref()).await
        })
        .await?;

        let runner = Arc::new(AgentGraphRunner::new(self.llm.clone(), checkpoints.clone()));
        let worker = spawn_worker(
            schema.clone(),
            engine.clone(),
            runner.clone(),
        );

        Ok(Arc::new(TenantWorkflowRuntime {
            schema,
            engine,
            checkpoints,
            runner,
            worker: Mutex::new(Some(worker)),
        }))
    }
}

async fn restore_engine(
    engine: &WorkflowEngine,
    persistence: &PostgresWorkflowPersistence,
) -> Result<(), String> {
    // 单机桌面应用：启动时释放上一次进程遗留的 ServiceTask 锁。
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}bpmn_engine_object
         SET worker_id = NULL, lock_expiration = NULL
         WHERE kind = 'service_task'",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&pool)
        .await
        .map_err(|e| format!("释放工作流任务锁失败: {e}"))?;

    let definitions = persistence
        .list_definitions()
        .await
        .map_err(|e| format!("加载流程定义失败: {e}"))?;
    for definition in definitions {
        engine.deploy_definition(definition).await;
    }

    let instances = persistence
        .list_instances()
        .await
        .map_err(|e| format!("加载流程实例失败: {e}"))?;
    for instance in instances {
        engine.restore_instance(instance).await;
    }
    for task in persistence
        .list_user_tasks()
        .await
        .map_err(|e| format!("加载用户任务失败: {e}"))?
    {
        engine.restore_user_task(task);
    }
    for task in persistence
        .list_service_tasks()
        .await
        .map_err(|e| format!("加载服务任务失败: {e}"))?
    {
        engine.restore_service_task(task);
    }
    for timer in persistence
        .list_timers()
        .await
        .map_err(|e| format!("加载定时器失败: {e}"))?
    {
        engine.restore_timer(timer);
    }
    for catch in persistence
        .list_message_catches()
        .await
        .map_err(|e| format!("加载消息事件失败: {e}"))?
    {
        engine.restore_message_catch(catch);
    }
    for catch in persistence
        .list_signal_catches()
        .await
        .map_err(|e| format!("加载信号事件失败: {e}"))?
    {
        engine.restore_signal_catch(catch);
    }
    for catch in persistence
        .list_conditional_catches()
        .await
        .map_err(|e| format!("加载条件事件失败: {e}"))?
    {
        engine.restore_conditional_catch(catch);
    }

    Ok(())
}

fn spawn_worker(
    schema: String,
    engine: Arc<WorkflowEngine>,
    runner: Arc<AgentGraphRunner>,
) -> JoinHandle<()> {
    let worker_id = format!("tauri-{}-{}", schema, Uuid::new_v4());
    tokio::spawn(async move {
        loop {
            let topics = vec![TOPIC_AGENT.to_string(), TOPIC_AGENT_RESUME.to_string()];
            let tasks = database::with_schema(schema.clone(), async {
                engine
                    .fetch_and_lock_service_tasks(&worker_id, 4, &topics, 600)
                    .await
            })
            .await;

            for task in tasks {
                let task_id = task.id;
                let result = database::with_schema(
                    schema.clone(),
                    process_service_task(&engine, &runner, &worker_id, task),
                )
                .await;
                if let Err(error) = result {
                    tracing::error!(
                        "工作流 Worker 处理任务失败: schema={}, task_id={}, error={}",
                        schema,
                        task_id,
                        error
                    );
                    let _ = engine
                        .fail_service_task(
                            task_id,
                            &worker_id,
                            Some(1),
                            Some(error.clone()),
                            None,
                        )
                        .await;
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    })
}

async fn process_service_task(
    engine: &WorkflowEngine,
    runner: &AgentGraphRunner,
    worker_id: &str,
    task: engine_core::runtime::PendingServiceTask,
) -> Result<(), String> {
    let snapshot = ExecutionSnapshot::from_variables(&task.variables_snapshot)
        .ok_or_else(|| "服务任务缺少 __workflow 快照".to_string())?;
    let node = snapshot
        .node_configs
        .get(&task.node_id)
        .ok_or_else(|| format!("服务任务节点 {} 不在执行快照中", task.node_id))?;
    let config = AgentConfig::from_value(&node.config);

    let execution_id = snapshot.execution_id.clone();
    let mut variables = task.variables_snapshot.clone();
    for key in [
        "__workflow",
        "business_key",
        "agent_status",
        "agent_thread_id",
        "agent_checkpoint_id",
        "agent_interrupt",
        "agent_approved",
        "approval_comment",
    ] {
        variables.remove(key);
    }

    let thread_id = task
        .variables_snapshot
        .get("agent_thread_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{}:{}", execution_id, task.node_id));

    let approval = if task.topic == TOPIC_AGENT_RESUME {
        let approved = task
            .variables_snapshot
            .get("agent_approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let comment = task
            .variables_snapshot
            .get("approval_comment")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let interrupt = task
            .variables_snapshot
            .get("agent_interrupt")
            .cloned()
            .unwrap_or(Value::Null);
        let mut payload = interrupt
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if let Value::Object(ref mut map) = payload {
            map.insert("approved".to_string(), json!(approved));
            map.insert("comment".to_string(), json!(comment));
        }
        Some(payload)
    } else {
        None
    };

    let _ = append_node_result(
        &execution_id,
        &task.node_id,
        Some(&node.node_type),
        "running",
        None,
        None,
        None,
    )
    .await;

    let started = Utc::now();
    let outcome = runner
        .run(AgentInvocation {
            thread_id: thread_id.clone(),
            node_id: task.node_id.clone(),
            config,
            input: Value::Object(
                variables
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
            approval,
        })
        .await;
    let duration = (Utc::now() - started).num_milliseconds();

    match outcome {
        AgentOutcome::Completed { output, .. } => {
            let mut complete = HashMap::new();
            complete.insert("agent_status".to_string(), json!("completed"));
            complete.insert("agent_output".to_string(), output.clone());
            complete.insert("result".to_string(), output.clone());
            complete.insert(format!("output_{}", task.node_id), output.clone());
            complete.insert("agent_thread_id".to_string(), json!(thread_id));
            complete.insert("agent_approved".to_string(), Value::Null);

            engine
                .complete_service_task(task.id, worker_id, complete)
                .await
                .map_err(|e| format!("完成 Agent 服务任务失败: {e}"))?;

            let _ = append_node_result(
                &execution_id,
                &task.node_id,
                Some(&node.node_type),
                "success",
                None,
                Some(&output),
                Some(duration),
            )
            .await;
            let _ = sync_execution(&execution_id).await;
            Ok(())
        }
        AgentOutcome::Interrupted {
            thread_id,
            checkpoint_id,
            payload,
        } => {
            let mut complete = HashMap::new();
            complete.insert("agent_status".to_string(), json!("waiting_approval"));
            complete.insert("agent_thread_id".to_string(), json!(thread_id));
            complete.insert("agent_checkpoint_id".to_string(), json!(checkpoint_id));
            complete.insert("agent_interrupt".to_string(), payload.clone());
            complete.insert("agent_approved".to_string(), Value::Null);

            engine
                .complete_service_task(task.id, worker_id, complete)
                .await
                .map_err(|e| format!("提交 Agent 审批中断失败: {e}"))?;

            let _ = append_node_result(
                &execution_id,
                &task.node_id,
                Some(&node.node_type),
                "waiting",
                None,
                Some(&payload),
                Some(duration),
            )
            .await;
            let _ = sync_execution(&execution_id).await;
            Ok(())
        }
        AgentOutcome::Failed(error) => {
            let _ = append_node_result(
                &execution_id,
                &task.node_id,
                Some(&node.node_type),
                "failed",
                None,
                None,
                Some(duration),
            )
            .await;
            Err(error)
        }
    }
}


