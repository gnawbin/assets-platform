//! 多用户审批组：成员快照、投票、终态判断。
//!
//! 引擎侧始终只有一个 UserTask（assignee=`group:<nodeId>`），
//! 本模块把引擎任务展开为多个成员待办，并在达到终态时通知调用方
//! 调用 `engine.complete_user_task`。

use crate::types::{
    ApprovalDecisionResult, ApprovalGroupPlan, ApprovalMember, PendingWorkflowTask,
};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// 审批组数据库行 + 实时票数。
#[derive(Debug, Clone)]
pub struct GroupRow {
    pub id: i64,
    pub execution_id: i64,
    pub instance_id: Uuid,
    pub engine_task_id: Uuid,
    pub node_id: String,
    pub approval_type: String,
    pub reject_action: String,
    pub members: Vec<ApprovalMember>,
    pub status: String,
    pub approved_count: i32,
    pub rejected_count: i32,
}

impl GroupRow {
    pub fn member(&self, user_id: i64) -> Option<&ApprovalMember> {
        let target = user_id.to_string();
        self.members
            .iter()
            .find(|member| member.user_id == target)
    }
}

/// 确保审批组行存在（按 engine_task_id 幂等）。
pub async fn ensure_group(
    execution_id: i64,
    instance_id: Uuid,
    engine_task_id: Uuid,
    node_id: &str,
    plan: &ApprovalGroupPlan,
) -> Result<GroupRow, String> {
    if let Some(group) = load_group(engine_task_id).await? {
        return Ok(group);
    }

    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let prefix = database::schema_prefix();
    let members = serde_json::to_value(&plan.members)
        .map_err(|e| format!("序列化审批组成员失败: {e}"))?;
    let id = utils::snowflake::next_id() as i64;
    let sql = format!(
        "INSERT INTO {prefix}workflow_approval_group
            (id, execution_id, instance_id, engine_task_id, node_id,
             approval_type, reject_action, members, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'open', NOW(), NOW())
         ON CONFLICT (engine_task_id) DO NOTHING",
        prefix = prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(execution_id)
        .bind(instance_id)
        .bind(engine_task_id)
        .bind(node_id)
        .bind(&plan.approval_type)
        .bind(&plan.reject_action)
        .bind(members)
        .execute(&pool)
        .await
        .map_err(|e| format!("创建审批组失败: {e}"))?;

    load_group_from(&pool, engine_task_id)
        .await?
        .ok_or_else(|| "审批组创建后读取失败".to_string())
}

/// 按引擎任务 ID 读取审批组。
pub async fn load_group(engine_task_id: Uuid) -> Result<Option<GroupRow>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    load_group_from(&pool, engine_task_id).await
}

async fn load_group_from(
    pool: &sqlx::PgPool,
    engine_task_id: Uuid,
) -> Result<Option<GroupRow>, String> {
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, execution_id, instance_id, engine_task_id, node_id,
                approval_type, reject_action, members, status
         FROM {prefix}workflow_approval_group
         WHERE engine_task_id = $1",
        prefix = prefix
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(engine_task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询审批组失败: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    build_group_row(pool, &row).await.map(Some)
}

async fn build_group_row(
    pool: &sqlx::PgPool,
    row: &sqlx::postgres::PgRow,
) -> Result<GroupRow, String> {
    let group_id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
    let members_value: Value = row.try_get("members").map_err(|e| e.to_string())?;
    let members: Vec<ApprovalMember> =
        serde_json::from_value(members_value).unwrap_or_default();
    let prefix = database::schema_prefix();
    let counts_sql = format!(
        "SELECT
            COUNT(*) FILTER (WHERE approved)::int AS approved_count,
            COUNT(*) FILTER (WHERE NOT approved)::int AS rejected_count
         FROM {prefix}workflow_approval_vote WHERE group_id = $1",
        prefix = prefix
    );
    let counts = sqlx::query(sqlx::AssertSqlSafe(counts_sql))
        .bind(group_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("统计审批票数失败: {e}"))?;

    Ok(GroupRow {
        id: group_id,
        execution_id: row.try_get("execution_id").map_err(|e| e.to_string())?,
        instance_id: row.try_get("instance_id").map_err(|e| e.to_string())?,
        engine_task_id: row.try_get("engine_task_id").map_err(|e| e.to_string())?,
        node_id: row.try_get("node_id").map_err(|e| e.to_string())?,
        approval_type: row.try_get("approval_type").map_err(|e| e.to_string())?,
        reject_action: row.try_get("reject_action").map_err(|e| e.to_string())?,
        members,
        status: row.try_get("status").map_err(|e| e.to_string())?,
        approved_count: counts.try_get("approved_count").unwrap_or(0),
        rejected_count: counts.try_get("rejected_count").unwrap_or(0),
    })
}

/// 审批人是否已经投票。
pub async fn has_voted(engine_task_id: Uuid, user_id: i64) -> Result<bool, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT EXISTS(
            SELECT 1 FROM {prefix}workflow_approval_vote
            WHERE engine_task_id = $1 AND user_id = $2
         )",
        prefix = prefix
    );
    sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(sql))
        .bind(engine_task_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("查询投票状态失败: {e}"))
}

/// 提交一票并返回终态判断。
///
/// 返回 `(结果, 最终是否通过)`；`None` 表示仍在等待其他成员。
pub async fn submit_vote(
    execution_id: i64,
    instance_id: Uuid,
    engine_task_id: Uuid,
    node_id: &str,
    user_id: i64,
    variables: &HashMap<String, Value>,
    plan: &ApprovalGroupPlan,
) -> Result<(ApprovalDecisionResult, Option<bool>), String> {
    let approved = variables
        .get("approved")
        .and_then(Value::as_bool)
        .ok_or_else(|| "审批结果缺少 approved 字段".to_string())?;
    let comment = variables
        .get("comment")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    ensure_group(execution_id, instance_id, engine_task_id, node_id, plan).await?;

    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let prefix = database::schema_prefix();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开启审批事务失败: {e}"))?;

    let lock_sql = format!(
        "SELECT id, execution_id, instance_id, engine_task_id, node_id,
                approval_type, reject_action, members, status
         FROM {prefix}workflow_approval_group
         WHERE engine_task_id = $1
         FOR UPDATE",
        prefix = prefix
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(lock_sql))
        .bind(engine_task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("锁定审批组失败: {e}"))?
        .ok_or_else(|| "审批组不存在".to_string())?;

    let group_id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
    let status: String = row.try_get("status").map_err(|e| e.to_string())?;
    let approval_type: String = row.try_get("approval_type").map_err(|e| e.to_string())?;
    let members_value: Value = row.try_get("members").map_err(|e| e.to_string())?;
    let members: Vec<ApprovalMember> =
        serde_json::from_value(members_value).unwrap_or_default();

    if status != "open" {
        return Err("该审批任务已结束".to_string());
    }
    let target = user_id.to_string();
    if !members.iter().any(|member| member.user_id == target) {
        return Err("您不是该审批节点的审批人".to_string());
    }

    let insert_sql = format!(
        "INSERT INTO {prefix}workflow_approval_vote
            (id, group_id, engine_task_id, user_id, approved, comment, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, NOW())",
        prefix = prefix
    );
    let insert = sqlx::query(sqlx::AssertSqlSafe(insert_sql))
        .bind(utils::snowflake::next_id() as i64)
        .bind(group_id)
        .bind(engine_task_id)
        .bind(user_id)
        .bind(approved)
        .bind(comment.as_deref())
        .execute(&mut *tx)
        .await;
    if let Err(error) = insert {
        if let sqlx::Error::Database(database_error) = &error {
            if database_error.code().as_deref() == Some("23505") {
                return Err("您已提交过审批，请勿重复操作".to_string());
            }
        }
        return Err(format!("写入审批投票失败: {error}"));
    }

    let counts_sql = format!(
        "SELECT
            COUNT(*) FILTER (WHERE approved)::int AS approved_count,
            COUNT(*) FILTER (WHERE NOT approved)::int AS rejected_count
         FROM {prefix}workflow_approval_vote WHERE group_id = $1",
        prefix = prefix
    );
    let counts = sqlx::query(sqlx::AssertSqlSafe(counts_sql))
        .bind(group_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("统计审批票数失败: {e}"))?;
    let approved_count: i32 = counts.try_get("approved_count").unwrap_or(0);
    let rejected_count: i32 = counts.try_get("rejected_count").unwrap_or(0);
    let member_count = members.len() as i32;

    let final_status = decide_status(
        &approval_type,
        approved_count,
        rejected_count,
        member_count,
    );
    if let Some(final_status) = final_status {
        let update_sql = format!(
            "UPDATE {prefix}workflow_approval_group
             SET status = $2, updated_at = NOW()
             WHERE id = $1",
            prefix = prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(update_sql))
            .bind(group_id)
            .bind(final_status)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("更新审批组状态失败: {e}"))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("提交审批事务失败: {e}"))?;

    let display_status = match final_status {
        Some("approved") => "completed",
        Some("rejected") => "rejected",
        _ => "pending",
    };
    Ok((
        ApprovalDecisionResult {
            status: display_status.to_string(),
            approved_count,
            rejected_count,
            member_count,
        },
        match final_status {
            Some("approved") => Some(true),
            Some("rejected") => Some(false),
            _ => None,
        },
    ))
}

fn decide_status(
    approval_type: &str,
    approved_count: i32,
    rejected_count: i32,
    member_count: i32,
) -> Option<&'static str> {
    if member_count <= 0 {
        return Some("rejected");
    }
    match approval_type {
        "all" => {
            if rejected_count > 0 {
                Some("rejected")
            } else if approved_count >= member_count {
                Some("approved")
            } else {
                None
            }
        }
        "any" => {
            if approved_count > 0 {
                Some("approved")
            } else if rejected_count >= member_count {
                Some("rejected")
            } else {
                None
            }
        }
        _ => {
            if approved_count > 0 {
                Some("approved")
            } else if rejected_count > 0 {
                Some("rejected")
            } else {
                None
            }
        }
    }
}

/// 读取审批组全部投票，用于写入流程变量 `approval_votes`。
pub async fn list_votes(engine_task_id: Uuid) -> Result<Vec<Value>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT user_id, approved, comment, created_at
         FROM {prefix}workflow_approval_vote
         WHERE engine_task_id = $1
         ORDER BY created_at ASC",
        prefix = prefix
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(engine_task_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询审批投票失败: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "user_id": row.try_get::<i64, _>("user_id").ok().map(|id| id.to_string()),
                "approved": row.try_get::<bool, _>("approved").unwrap_or(false),
                "comment": row.try_get::<Option<String>, _>("comment").ok().flatten(),
                "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").ok(),
            })
        })
        .collect())
}

/// 构造审批组待办（供 `list_pending_tasks` 使用）。
pub fn group_pending_task(
    group: &GroupRow,
    task_id: Uuid,
    user: &ApprovalMember,
    has_voted: bool,
) -> PendingWorkflowTask {

    PendingWorkflowTask {
        task_id: task_id.to_string(),
        instance_id: group.instance_id.to_string(),
        node_id: group.node_id.clone(),
        assignee: user.user_id.clone(),
        created_at: chrono::Utc::now(),
        business_key: None,
        execution_id: Some(group.execution_id.to_string()),
        workflow_id: None,
        workflow_name: None,
        interrupt_payload: None,
        assignee_name: Some(user.user_name.clone()),
        approval_type: Some(group.approval_type.clone()),
        member_count: Some(group.members.len() as i32),
        approved_count: Some(group.approved_count),
        rejected_count: Some(group.rejected_count),
        has_voted: Some(has_voted),
    }
}




#[cfg(test)]
mod tests {
    use super::decide_status;

    #[test]
    fn all_requires_every_approval() {
        assert_eq!(decide_status("all", 1, 0, 2), None);
        assert_eq!(decide_status("all", 2, 0, 2), Some("approved"));
        assert_eq!(decide_status("all", 1, 1, 2), Some("rejected"));
    }

    #[test]
    fn any_requires_one_approval_or_all_rejections() {
        assert_eq!(decide_status("any", 0, 1, 2), None);
        assert_eq!(decide_status("any", 1, 1, 2), Some("approved"));
        assert_eq!(decide_status("any", 0, 2, 2), Some("rejected"));
    }

    #[test]
    fn single_finishes_on_first_vote() {
        assert_eq!(decide_status("single", 1, 0, 1), Some("approved"));
        assert_eq!(decide_status("single", 0, 1, 1), Some("rejected"));
    }
}
