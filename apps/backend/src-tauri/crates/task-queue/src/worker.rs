//! 消费者：worker 装配与任务执行
//!
//! 三条硬约束（设计文档 §9 / §10.1）：
//! 1. 任务体必须整体包在 `database::with_schema(task.tenant, ..)` 里，否则会写到
//!    `GLOBAL_SCHEMA`（串租户）；
//! 2. CPU 密集的上传/解析不在 async 线程上跑：本地解析走 `parser`（内部 `spawn_blocking`），
//!    音视频走 Python 侧车；重任务额外受进程级信号量限制；
//! 3. 长任务必须 `long_running()`，否则会被判定为孤儿并被别的实例重复领取。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use apalis::prelude::{
    BoxDynError, Data, LongRunningExt, WorkerBuilder, WorkerBuilderExt, WorkerContext, WorkerError,
};
use apalis_postgres::{Config, PostgresStorage};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;

use crate::config::TaskQueueConfig;
use crate::error::TaskError;
use crate::job::{ParseTask, QUEUE_PARSE, QUEUE_PARSE_VIDEO, QUEUE_VECTORIZE, VectorizeTask};
use crate::ticket;

/// 向量化队列每实例 worker 数（embedding 走网络，可并发）
const VECTORIZE_CONCURRENCY: usize = 2;

/// 重任务（音视频）进程级并发上限：ffmpeg + 模型推理的内存峰值压制
const HEAVY_TASK_PERMITS: usize = 1;

/// worker 依赖（由调用方注入，避免本 crate 反向依赖 `api` / `gateway`）
pub struct TaskDeps {
    /// 写连接池（`apalis` 与业务写都走它）
    pub pool: sqlx::PgPool,
    /// 解析结果落库钩子（业务侧实现，见 [`ParseResultSink`]）
    pub sink: Arc<dyn ParseResultSink>,
    /// 重任务并发闸门
    pub heavy_permits: Semaphore,
}

impl TaskDeps {
    /// 构造依赖：默认使用只记日志的 [`LoggingSink`]
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            sink: Arc::new(LoggingSink),
            heavy_permits: Semaphore::new(HEAVY_TASK_PERMITS),
        }
    }

    /// 替换解析结果落库实现（链式构造）
    #[must_use]
    pub fn with_sink(mut self, sink: Arc<dyn ParseResultSink>) -> Self {
        self.sink = sink;
        self
    }
}

/// 解析结果落库钩子
///
/// `task-queue` 不感知业务表结构（`knowledge_asset` 等），因此把「结果往哪写」交给业务侧：
/// 实现方在钩子里用 `database::schema_prefix()` 拼自己的租户表即可
/// （调用时已在 `with_schema` 上下文内）。
pub trait ParseResultSink: Send + Sync {
    /// 解析成功后回调（向量化已在入队任务内完成，这里只处理解析产物）
    fn on_parsed<'a>(
        &'a self,
        task: &'a ParseTask,
        result: &'a parser::ParseResult,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskError>> + Send + 'a>>;
}

/// 默认实现：只记日志（P1 占位；业务侧注入真实实现后再落 `task_ticket` / 资产表）
pub struct LoggingSink;

impl ParseResultSink for LoggingSink {
    fn on_parsed<'a>(
        &'a self,
        task: &'a ParseTask,
        result: &'a parser::ParseResult,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskError>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                asset_id = task.asset_id,
                file_type = %result.file_type,
                text_len = result.raw_text.chars().count() as u64,
                images = result.images.len() as u64,
                "解析完成（未配置 ParseResultSink，结果仅记日志）"
            );
            Ok(())
        })
    }
}

/// 按配置启动全部 worker，返回句柄（调用方可 `abort` 或在关闭信号后等待结束）
#[allow(
    clippy::needless_pass_by_value,
    reason = "保持调用方 API 简洁：deps/cfg/shutdown 由 start_with_deps 原样传入，worker 内再按队列分发"
)]
pub fn spawn_all(
    deps: Arc<TaskDeps>,
    cfg: TaskQueueConfig,
    shutdown: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::new();

    for index in 0..cfg.parse_concurrency {
        handles.push(spawn_parse(
            deps.clone(),
            &cfg,
            QUEUE_PARSE,
            index,
            shutdown.clone(),
        ));
    }

    if cfg.video_enabled {
        // 视频/音频：并发固定 1，且 `batch_size` 固定 1（公平性，见设计文档 §10.3）
        handles.push(spawn_parse(
            deps.clone(),
            &cfg,
            QUEUE_PARSE_VIDEO,
            0,
            shutdown.clone(),
        ));
    }

    for index in 0..VECTORIZE_CONCURRENCY {
        handles.push(spawn_vectorize(deps.clone(), &cfg, index, shutdown.clone()));
    }

    tracing::info!(
        parse = cfg.parse_concurrency as u64,
        video = cfg.video_enabled,
        vectorize = VECTORIZE_CONCURRENCY as u64,
        instance_id = %cfg.instance_id,
        "任务队列 worker 已启动"
    );
    handles
}

/// 启动一个解析 worker
fn spawn_parse(
    deps: Arc<TaskDeps>,
    cfg: &TaskQueueConfig,
    queue: &'static str,
    index: usize,
    shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let name = format!("{queue}-{}-{index}", cfg.instance_id);
    let batch_size = if queue == QUEUE_PARSE_VIDEO {
        1
    } else {
        cfg.batch_size
    };
    let backend = PostgresStorage::new(&deps.pool)
        .with_config(Config::default().queue(queue).batch_size(batch_size));
    let worker = WorkerBuilder::new(name.clone())
        .backend(backend)
        .data(deps)
        .long_running()
        .enable_tracing()
        .build(handle_parse);

    tokio::spawn(async move {
        let signal = shutdown_signal(shutdown);
        if let Err(e) = worker.run_until(signal).await {
            tracing::error!(worker = %name, error = %e, "解析 worker 异常退出");
        } else {
            tracing::info!(worker = %name, "解析 worker 已优雅退出");
        }
    })
}

/// 启动一个向量化 worker
fn spawn_vectorize(
    deps: Arc<TaskDeps>,
    cfg: &TaskQueueConfig,
    index: usize,
    shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let name = format!("{QUEUE_VECTORIZE}-{}-{index}", cfg.instance_id);
    let backend = PostgresStorage::new(&deps.pool)
        .with_config(Config::default().queue(QUEUE_VECTORIZE).batch_size(4));
    let worker = WorkerBuilder::new(name.clone())
        .backend(backend)
        .data(deps)
        .long_running()
        .enable_tracing()
        .build(handle_vectorize);

    tokio::spawn(async move {
        let signal = shutdown_signal(shutdown);
        if let Err(e) = worker.run_until(signal).await {
            tracing::error!(worker = %name, error = %e, "向量化 worker 异常退出");
        } else {
            tracing::info!(worker = %name, "向量化 worker 已优雅退出");
        }
    })
}

/// `watch` 关闭信号 → apalis 的 `run_until` 信号（Resolve 即优雅停止领取新任务）
async fn shutdown_signal(mut shutdown: watch::Receiver<bool>) -> Result<(), WorkerError> {
    let _ = shutdown.changed().await;
    Ok(())
}

/// 任务进度上报（票据写失败只记日志，绝不影响任务本身）
#[derive(Clone, Copy)]
struct Progress<'a> {
    pool: &'a sqlx::PgPool,
    schema: &'a str,
    task_id: Option<&'a str>,
}

impl Progress<'_> {
    /// 推进阶段（`downloading` / `parsing` / `chunking`）
    async fn stage(&self, stage: &str, percent: i16) {
        let Some(task_id) = self.task_id else {
            return;
        };
        if let Err(e) = ticket::mark_stage(self.pool, self.schema, task_id, stage, percent).await {
            tracing::debug!(error = %e, "更新任务进度失败（忽略）");
        }
    }
}

/// 解析任务处理器
///
/// 注册为 `WorkerBuilder::build(handle_parse)`；参数注入由 apalis 完成：
/// `ParseTask`（任务参数）、`Data<Arc<TaskDeps>>`（`builder.data(..)` 注入）、`WorkerContext`。
async fn handle_parse(
    task: ParseTask,
    deps: Data<Arc<TaskDeps>>,
    worker: WorkerContext,
) -> Result<(), BoxDynError> {
    if let Err(e) = task.validate() {
        tracing::warn!(asset_id = task.asset_id, error = %e, "解析任务参数非法，直接终止（不重试）");
        return Err(e.into_box());
    }

    // 重任务（音视频）先抢进程级闸门：ffmpeg + 模型推理的内存峰值必须压住
    let _permit = if task.doc_type.is_heavy() {
        match deps.heavy_permits.acquire().await {
            Ok(permit) => Some(permit),
            Err(e) => {
                return Err(TaskError::Transient(format!("重任务信号量已关闭: {e}")).into_box());
            }
        }
    } else {
        None
    };

    let tenant = task.tenant.clone();
    let asset_id = task.asset_id;
    let task_id = task.task_id.clone();
    if let Some(id) = task_id.as_deref()
        && let Err(e) = ticket::mark_running(&deps.pool, &tenant, id, "parsing", 10).await
    {
        tracing::warn!(error = %e, "更新任务票据为 running 失败（忽略）");
    }
    tracing::info!(
        worker = %worker.name(),
        asset_id,
        tenant = %tenant,
        doc_type = task.doc_type.as_str(),
        "开始解析任务"
    );

    let pool = deps.pool.clone();
    let deps = Arc::clone(&deps.0);
    let progress = Progress {
        pool: &pool,
        schema: &tenant,
        task_id: task_id.as_deref(),
    };

    match database::with_schema(tenant.clone(), run_parse(task, deps, progress)).await {
        Ok(outcome) => {
            if let Some(id) = task_id.as_deref()
                && let Err(e) = ticket::mark_done(&pool, &tenant, id).await
            {
                tracing::warn!(error = %e, "标记任务完成失败（忽略）");
            }
            tracing::info!(
                asset_id,
                text_len = outcome.text_len as u64,
                chunks = outcome.chunks as u64,
                "解析任务完成"
            );
            Ok(())
        }
        Err(e) => {
            if let Some(id) = task_id.as_deref()
                && let Err(err) = ticket::mark_failed(&pool, &tenant, id, &e.to_string()).await
            {
                tracing::warn!(error = %err, "标记任务失败状态失败（忽略）");
            }
            tracing::warn!(
                asset_id,
                retryable = e.is_retryable(),
                error = %e,
                "解析任务失败"
            );
            Err(e.into_box())
        }
    }
}

/// 向量化任务处理器
async fn handle_vectorize(
    task: VectorizeTask,
    deps: Data<Arc<TaskDeps>>,
    worker: WorkerContext,
) -> Result<(), BoxDynError> {
    if let Err(e) = task.validate() {
        tracing::warn!(asset_id = task.asset_id, error = %e, "向量化任务参数非法，直接终止");
        return Err(e.into_box());
    }

    let tenant = task.tenant.clone();
    let asset_id = task.asset_id;
    let task_id = task.task_id.clone();
    if let Some(id) = task_id.as_deref()
        && let Err(e) = ticket::mark_running(&deps.pool, &tenant, id, "chunking", 30).await
    {
        tracing::warn!(error = %e, "更新任务票据为 running 失败（忽略）");
    }
    tracing::info!(worker = %worker.name(), asset_id, tenant = %tenant, "开始向量化任务");

    match database::with_schema(tenant.clone(), run_vectorize(task)).await {
        Ok(chunks) => {
            if let Some(id) = task_id.as_deref()
                && let Err(e) = ticket::mark_done(&deps.pool, &tenant, id).await
            {
                tracing::warn!(error = %e, "标记任务完成失败（忽略）");
            }
            tracing::info!(asset_id, chunks = chunks as u64, "向量化任务完成");
            Ok(())
        }
        Err(e) => {
            if let Some(id) = task_id.as_deref()
                && let Err(err) = ticket::mark_failed(&deps.pool, &tenant, id, &e.to_string()).await
            {
                tracing::warn!(error = %err, "标记任务失败状态失败（忽略）");
            }
            tracing::warn!(asset_id, retryable = e.is_retryable(), error = %e, "向量化任务失败");
            Err(e.into_box())
        }
    }
}

/// 解析任务执行体（**必须在 `with_schema` 内调用**）
struct ParseOutcome {
    text_len: usize,
    chunks: usize,
}

async fn run_parse(
    task: ParseTask,
    deps: Arc<TaskDeps>,
    progress: Progress<'_>,
) -> Result<ParseOutcome, TaskError> {
    let work_dir = TempDir::new()?;
    progress.stage("downloading", 20).await;
    let local = localize(&task, work_dir.path()).await?;

    // local（进程内 Rust）/ remote（Python 侧车）由 DOC_PARSER_BACKEND 决定，本 crate 不感知
    progress.stage("parsing", 40).await;
    let client = service::doc_parser::DocParserClient::new();
    let local_path = local.to_string_lossy().to_string();
    let result = client
        .parse_file(&local_path, &task.options)
        .await
        .map_err(TaskError::from_parse_error)?;

    let chunks = if task.skip_index() || result.raw_text.trim().is_empty() {
        0
    } else {
        progress.stage("chunking", 80).await;
        service::rag_service::RAGRetriever::chunk_and_vectorize(
            task.asset_id,
            &result.raw_text,
            &task.title,
            &task.okf_type,
            &task.tags,
            task.tree_node_id,
        )
        .await
        .map_err(|e| TaskError::Transient(format!("分块/向量化失败: {e}")))?
        .len()
    };

    deps.sink.on_parsed(&task, &result).await?;

    Ok(ParseOutcome {
        text_len: result.raw_text.chars().count(),
        chunks,
    })
}

/// 向量化任务执行体（**必须在 `with_schema` 内调用**）
async fn run_vectorize(task: VectorizeTask) -> Result<usize, TaskError> {
    let inserted = service::rag_service::RAGRetriever::chunk_and_vectorize(
        task.asset_id,
        &task.text,
        &task.title,
        &task.okf_type,
        &task.tags,
        task.tree_node_id,
    )
    .await
    .map_err(|e| TaskError::Transient(format!("分块/向量化失败: {e}")))?;
    Ok(inserted.len())
}

/// 把 `storage_url` 变成 worker 可读的本地文件
///
/// - `s3://bucket/key` → 下载到任务私有临时目录（随 [`TempDir`] 清理）；
/// - `file:///abs/path` / 绝对路径 → 直接使用（仅桌面本地调试，入队前已校验存在性）。
async fn localize(task: &ParseTask, work_dir: &Path) -> Result<PathBuf, TaskError> {
    let url = task.storage_url.trim();

    if let Some(rest) = url.strip_prefix("s3://") {
        let (bucket, key) = split_s3(rest)?;
        let s3 = storage::s3::S3Client::from_env()
            .await
            .map_err(|e| TaskError::Transient(format!("S3 客户端初始化失败: {e}")))?;
        let local = s3
            .download_object(&bucket, &key, work_dir)
            .await
            .map_err(|e| {
                TaskError::Transient(format!("S3 下载失败（s3://{bucket}/{key}）: {e}"))
            })?;
        return Ok(local);
    }

    if let Some(rest) = url.strip_prefix("file://") {
        return Ok(PathBuf::from(rest));
    }

    Ok(PathBuf::from(url))
}

/// 拆分 `bucket/key`（key 允许包含 `/`）
fn split_s3(rest: &str) -> Result<(String, String), TaskError> {
    let mut parts = rest.splitn(2, '/');
    let bucket = parts.next().unwrap_or_default().trim();
    let key = parts.next().unwrap_or_default().trim();
    if bucket.is_empty() || key.is_empty() {
        return Err(TaskError::Invalid(format!(
            "S3 引用缺少 bucket/key: s3://{rest}"
        )));
    }
    Ok((bucket.to_string(), key.to_string()))
}

/// 任务私有临时目录（`Drop` 时递归删除）
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, TaskError> {
        let dir = std::env::temp_dir().join(format!("task-queue-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).map_err(|e| {
            TaskError::Transient(format!("创建任务临时目录失败（{}）: {e}", dir.display()))
        })?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0)
            && self.0.exists()
        {
            tracing::warn!(
                dir = %self.0.display(),
                error = %e,
                "清理任务临时目录失败（磁盘水位需关注）"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_s3_keeps_nested_key() {
        let (bucket, key) = split_s3("assets-platform/knowledge/2026/a.mp4").expect("❌ 解析失败");
        assert_eq!(bucket, "assets-platform");
        assert_eq!(key, "knowledge/2026/a.mp4");
    }

    #[test]
    fn test_split_s3_rejects_missing_key() {
        assert!(matches!(
            split_s3("bucket-only"),
            Err(TaskError::Invalid(_))
        ));
        assert!(matches!(split_s3("/key"), Err(TaskError::Invalid(_))));
        assert!(matches!(split_s3("bucket/"), Err(TaskError::Invalid(_))));
    }

    #[test]
    fn test_temp_dir_is_removed_on_drop() {
        let path = {
            let dir = TempDir::new().expect("❌ 创建临时目录失败");
            let path = dir.path().to_path_buf();
            assert!(path.is_dir());
            path
        };
        assert!(!path.exists(), "❌ 任务临时目录必须在 Drop 时删除");
    }
}
