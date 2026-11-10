//! 端点归一化与凭证映射（纯函数，便于单测）
//!
//! LanceDB 的 `connect` 接受三类 URI，本模块统一成它认识的写法：
//! - 本地目录：`/data/lance`、`D:\data\lance`、`file:///data/lance`；
//! - 对象存储：`s3://bucket/prefix`、`gs://bucket/prefix`、`az://container/prefix`；
//! - LanceDB Cloud：`db://...`（需要 lancedb 的 `remote` feature，本构建未启用）。
//!
//! 本地目录不做 `file://` 包装：Windows 上 `file://` 的盘符语义容易踩坑，
//! 直接传绝对路径最稳。

use vector::{StoreAuth, StoreConfig, VectorError, VectorResult};

/// 本地目录 scheme
const LOCAL_SCHEMES: [&str; 1] = ["file"];
/// 对象存储 scheme（object_store 支持的前缀）
const CLOUD_SCHEMES: [&str; 7] = ["s3", "gs", "gcs", "az", "azure", "abfs", "abfss"];
/// LanceDB Cloud scheme
const CLOUD_DB_SCHEMES: [&str; 1] = ["db"];

/// 把 [`StoreConfig`] 的端点归一化成 lancedb 可用的 URI
pub fn store_uri(config: &StoreConfig) -> VectorResult<String> {
    config.validate()?;
    let endpoint = config.endpoint.trim();
    if endpoint.is_empty() {
        return Err(VectorError::config(
            "LanceDB 端点不能为空（本地目录 / file:///data/lance / s3://bucket/prefix）",
        ));
    }
    let scheme = config.scheme().to_ascii_lowercase();
    if scheme.is_empty() {
        return Ok(trim_trailing_separator(endpoint));
    }
    let rest = endpoint
        .split_once("://")
        .map_or(endpoint, |(_, rest)| rest);
    if LOCAL_SCHEMES.contains(&scheme.as_str()) {
        let path = strip_drive_slash(rest);
        if path.trim().is_empty() {
            return Err(VectorError::config(format!(
                "file:// 后面必须跟本地目录：{endpoint}"
            )));
        }
        return Ok(trim_trailing_separator(path));
    }
    if CLOUD_SCHEMES.contains(&scheme.as_str()) {
        // 对象存储 URI 原样交给 lancedb / object_store
        return Ok(endpoint.to_string());
    }
    if CLOUD_DB_SCHEMES.contains(&scheme.as_str()) {
        return Err(VectorError::unsupported(
            "LanceDB Cloud（db://）需要启用 lancedb 的 `remote` feature，本构建只支持本地目录与对象存储",
        ));
    }
    Err(VectorError::config(format!(
        "LanceDB 端点 scheme 必须是 file:// / s3:// / gs:// / az://，实际：{scheme}"
    )))
}

/// 是否为本地目录（决定是否需要预建目录、是否算「嵌入式」）
pub fn is_local(uri: &str) -> bool {
    let lower = uri.trim().to_ascii_lowercase();
    if LOCAL_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(&format!("{scheme}://")))
    {
        return true;
    }
    !lower.contains("://")
}

/// 去掉末尾的分隔符（`/data/lance/` → `/data/lance`）
fn trim_trailing_separator(path: &str) -> String {
    let trimmed = path.trim();
    // 根目录（`/`、`C:\`）保留原样
    if trimmed.len() <= 1 {
        return trimmed.to_string();
    }
    let without = trimmed.trim_end_matches(['/', '\\']);
    if without.is_empty() {
        trimmed.to_string()
    } else if without.ends_with(':') {
        // `C:`（盘符）补回分隔符，避免 lancedb 把它当成相对路径
        format!("{without}\\")
    } else {
        without.to_string()
    }
}

/// `file:///C:/data` → `C:/data`；`file:///data` → `/data`
fn strip_drive_slash(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        &rest[1..]
    } else {
        rest
    }
}

/// 对象存储凭证 → object_store 的 storage options
///
/// 只有 `AK/SK` 会变成参数；用户名密码与令牌对 LanceDB 没有意义，直接报错，
/// 避免界面上填了却静默失效。
pub fn storage_options(auth: &StoreAuth) -> VectorResult<Vec<(String, String)>> {
    match auth {
        StoreAuth::None => Ok(Vec::new()),
        StoreAuth::AwsKey {
            access_key,
            secret_key,
            session_token,
            region,
            endpoint,
        } => {
            let mut options = vec![
                ("aws_access_key_id".to_string(), access_key.clone()),
                ("aws_secret_access_key".to_string(), secret_key.clone()),
            ];
            if let Some(token) = session_token.as_ref().filter(|value| !value.is_empty()) {
                options.push(("aws_session_token".to_string(), token.clone()));
            }
            if let Some(region) = region.as_ref().filter(|value| !value.is_empty()) {
                options.push(("aws_region".to_string(), region.clone()));
            }
            if let Some(endpoint) = endpoint.as_ref().filter(|value| !value.is_empty()) {
                options.push(("aws_endpoint".to_string(), endpoint.clone()));
            }
            Ok(options)
        }
        StoreAuth::Password { .. } => Err(VectorError::unsupported(
            "LanceDB 本地目录 / 对象存储不支持用户名密码认证，请改用 AK/SK",
        )),
        StoreAuth::Token { .. } => Err(VectorError::unsupported(
            "LanceDB Cloud 令牌认证需要启用 lancedb 的 `remote` feature",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(endpoint: &str) -> StoreConfig {
        StoreConfig::new(endpoint)
    }

    #[test]
    fn local_paths_are_normalized() {
        assert_eq!(store_uri(&config("/data/lance")).unwrap(), "/data/lance");
        // 末尾分隔符被去掉（Windows / Unix 都测）
        assert_eq!(store_uri(&config("/data/lance/")).unwrap(), "/data/lance");
        assert_eq!(
            store_uri(&config("D:\\data\\lance\\")).unwrap(),
            "D:\\data\\lance"
        );
        // file:// 去掉 scheme，Windows 盘符前的斜杠一并去掉
        assert_eq!(
            store_uri(&config("file:///data/lance")).unwrap(),
            "/data/lance"
        );
        assert_eq!(
            store_uri(&config("file:///D:/data/lance")).unwrap(),
            "D:/data/lance"
        );
        assert!(is_local("/data/lance"));
        assert!(is_local("file:///data/lance"));
    }

    #[test]
    fn cloud_uris_pass_through() {
        for uri in [
            "s3://bucket/prefix",
            "gs://bucket/prefix",
            "az://container/prefix",
        ] {
            assert_eq!(store_uri(&config(uri)).unwrap(), uri);
            assert!(!is_local(uri));
        }
    }

    #[test]
    fn unsupported_endpoints_are_rejected() {
        // LanceDB Cloud 需要 remote feature
        assert_eq!(
            store_uri(&config("db://my-db")).unwrap_err().kind(),
            "unsupported"
        );
        // 其它 scheme 一律拒绝
        assert_eq!(
            store_uri(&config("redis://cache:6379"))
                .unwrap_err()
                .kind(),
            "config"
        );
        assert_eq!(store_uri(&config("   ")).unwrap_err().kind(), "config");
        assert_eq!(store_uri(&config("file://")).unwrap_err().kind(), "config");
    }

    #[test]
    fn ak_sk_becomes_storage_options() {
        let auth = StoreAuth::aws_key("AK", "SK")
            .with_region("us-east-1")
            .with_session_token("TOKEN");
        let options = storage_options(&auth).unwrap();
        assert!(options.contains(&("aws_access_key_id".to_string(), "AK".to_string())));
        assert!(options.contains(&("aws_secret_access_key".to_string(), "SK".to_string())));
        assert!(options.contains(&("aws_region".to_string(), "us-east-1".to_string())));
        assert!(options.contains(&("aws_session_token".to_string(), "TOKEN".to_string())));

        assert!(storage_options(&StoreAuth::None).unwrap().is_empty());
        assert_eq!(
            storage_options(&StoreAuth::password("u", "p"))
                .unwrap_err()
                .kind(),
            "unsupported"
        );
        assert_eq!(
            storage_options(&StoreAuth::token("t")).unwrap_err().kind(),
            "unsupported"
        );
    }
}

