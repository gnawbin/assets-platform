//! `.env.toml` 配置加载（桌面 / 无头服务共用）
//!
//! 解析规则与改造前的 `apps/backend/src-tauri/src/lib.rs::load_env` 完全一致：
//! `[section]` 下的 `key` 映射为环境变量 `SECTION_KEY`，并对两处旧名做兼容：
//! - `[postgres]` 段额外映射一份 `PG_*`
//! - `[auth] jwt_secret` 额外映射 `JWT_SECRET`
//! - `[postgres] read_replicas` 数组拼接为 `PG_READ_HOSTS` / `POSTGRES_READ_HOSTS`
//!
//! 与运行形态的差异只在「去哪里找文件」：由调用方传入候选路径，本模块不感知
//! 桌面或服务端，避免把 `CARGO_MANIFEST_DIR` 这类编译期路径写死在这里。

use std::path::{Path, PathBuf};

/// 按顺序查找并加载第一个可读的 `.env.toml`
///
/// 行为约定（与改造前一致）：
/// - 命中第一个**可读**的文件即停止，不再尝试后续候选（即使该文件解析失败）
/// - 全部候选都不可读时只记录告警，不中断启动（各 crate 均有环境变量兜底默认值）
///
/// # 返回
/// 实际加载成功的文件路径；未找到或解析失败时返回 `None`。
pub fn load_env_toml<P: AsRef<Path>>(candidates: &[P]) -> Option<PathBuf> {
    for candidate in candidates {
        let path = candidate.as_ref();

        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };

        return match text.parse::<toml::Table>() {
            Ok(table) => {
                apply_table(&table);
                tracing::info!(config = %path.display(), "已加载 .env TOML 配置文件");
                Some(path.to_path_buf())
            }
            Err(e) => {
                tracing::warn!(config = %path.display(), "配置文件解析失败: {}", e);
                None
            }
        };
    }

    tracing::warn!("未找到 .env TOML 配置文件，将使用环境变量与各模块默认值");
    None
}

/// 把解析后的 TOML 表写入进程环境变量
fn apply_table(table: &toml::Table) {
    for (section, values) in table {
        let Some(sub_table) = values.as_table() else {
            continue;
        };

        // 特殊处理 postgres 段的 read_replicas 数组 → host:port:weight,...
        if section == "postgres" {
            if let Some(toml::Value::Array(replicas)) = sub_table.get("read_replicas") {
                let hosts_str: Vec<String> = replicas
                    .iter()
                    .filter_map(|v| {
                        let table = v.as_table()?;
                        let host = table.get("host")?.as_str()?;
                        let port = table
                            .get("port")
                            .and_then(|p| p.as_integer())
                            .unwrap_or(5432);
                        let weight = table
                            .get("weight")
                            .and_then(|w| w.as_integer())
                            .unwrap_or(1);
                        Some(format!("{}:{}:{}", host, port, weight))
                    })
                    .collect();
                if !hosts_str.is_empty() {
                    let pg_read_hosts = hosts_str.join(",");
                    // SAFETY: 启动期配置加载，此时尚未有其他线程并发读取环境变量。
                    unsafe {
                        std::env::set_var("PG_READ_HOSTS", &pg_read_hosts);
                        std::env::set_var("POSTGRES_READ_HOSTS", &pg_read_hosts);
                    }
                }
            }
        }

        for (key, value) in sub_table {
            let env_value = match value {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Float(f) => f.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                _ => continue,
            };

            // 生成标准环境变量名：SECTION_KEY
            let env_key = format!("{}_{}", section.to_uppercase(), key.to_uppercase());
            // SAFETY: 同上，启动期配置加载，无并发读取环境变量的线程。
            unsafe { std::env::set_var(&env_key, &env_value) };

            // 兼容旧版环境变量名
            if section == "postgres" {
                let legacy_key = format!("PG_{}", key.to_uppercase());
                // SAFETY: 同上。
                unsafe { std::env::set_var(&legacy_key, &env_value) };
            } else if section == "auth" && key == "jwt_secret" {
                // SAFETY: 同上。
                unsafe { std::env::set_var("JWT_SECRET", &env_value) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未找到任何候选文件时不应 panic，只返回 None
    #[test]
    fn missing_candidates_return_none() {
        let candidates = [
            PathBuf::from("__not_exists__/a.toml"),
            PathBuf::from("__not_exists__/b.toml"),
        ];
        assert!(load_env_toml(&candidates).is_none());
    }

    /// 环境变量名映射规则：`[api] port` → `API_PORT`
    #[test]
    fn section_key_maps_to_env_var() {
        let table: toml::Table = "[api]\nport = 3999\n".parse().expect("示例 TOML 应可解析");
        apply_table(&table);
        assert_eq!(std::env::var("API_PORT").ok().as_deref(), Some("3999"));
    }
}
