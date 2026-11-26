//! OAuth 方案 B 的浏览器会话配套（Cookie / CSRF / 同意页）
//!
//! 设计文档 §7.3：方案 A 用 Bearer 调 `/oauth/authorize`（非标准），
//! 方案 B 用 **httpOnly 会话 Cookie + 浏览器重定向**，才符合 OAuth 2.1。
//! 本模块只放 HTTP 层的东西（Cookie 序列化 / 解析、表单解析、HTML 渲染、
//! 客户端 IP），数据访问在 `service::session_service`。
//!
//! 安全要点：
//! - 会话 Cookie：`HttpOnly` + `SameSite=Lax` + `Path=/`，TLS 下额外 `Secure`；
//! - CSRF：双提交 Cookie（非 HttpOnly 的 `ap_csrf` + 表单隐藏域），常数时间比较；
//! - 同意页所有插值都经 [`html_escape`]，避免客户端名称 / scope 造成 XSS。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use axum::http::{header, HeaderMap};

/// 会话 Cookie 名（httpOnly，JS 读不到）
pub const SESSION_COOKIE_NAME: &str = "ap_session";

/// CSRF Cookie 名（必须可被同源表单读取，故不加 HttpOnly）
pub const CSRF_COOKIE_NAME: &str = "ap_csrf";

/// 全站 Cookie 的 `Path`（授权端点在根路径，不能用 `/oauth`）
const COOKIE_PATH: &str = "/";

/// 是否给 Cookie 加 `Secure`（设计文档 §8.2：只有显式允许明文时才不加）
pub fn cookie_secure() -> bool {
    fn truthy(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }
    match std::env::var("OAUTH_ALLOW_INSECURE_HTTP") {
        Ok(value) => !truthy(&value),
        Err(_) => true,
    }
}

/// 序列化会话 Cookie（HttpOnly；明文只在 Set-Cookie 里出现一次）
pub fn build_session_cookie(token: &str, max_age_secs: i64) -> String {
    let mut cookie = format!(
        "{}={}; Path={}; Max-Age={}; HttpOnly; SameSite=Lax",
        SESSION_COOKIE_NAME,
        token,
        COOKIE_PATH,
        max_age_secs.max(0)
    );
    if cookie_secure() {
        cookie.push_str("; Secure");
    }
    cookie
}

/// 序列化 CSRF Cookie（**不加** HttpOnly：双提交校验需要同源脚本读取）
pub fn build_csrf_cookie(token: &str, max_age_secs: i64) -> String {
    let mut cookie = format!(
        "{}={}; Path={}; Max-Age={}; SameSite=Lax",
        CSRF_COOKIE_NAME,
        token,
        COOKIE_PATH,
        max_age_secs.max(0)
    );
    if cookie_secure() {
        cookie.push_str("; Secure");
    }
    cookie
}

/// 清理会话 Cookie（登出 / 会话失效时下发）
pub fn clear_cookie(name: &str) -> String {
    format!(
        "{}=; Path={}; Max-Age=0; HttpOnly; SameSite=Lax",
        name, COOKIE_PATH
    )
}

/// 清理 CSRF Cookie（非 HttpOnly，与写入时属性保持一致）
pub fn clear_csrf_cookie() -> String {
    format!(
        "{}=; Path={}; Max-Age=0; SameSite=Lax",
        CSRF_COOKIE_NAME, COOKIE_PATH
    )
}

/// 从 `Cookie` 请求头读取指定名称的值（RFC 6265 §4.2.1）
pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for pair in raw.split(';') {
            let mut parts = pair.splitn(2, '=');
            let Some(key) = parts.next() else { continue };
            let val = parts.next().unwrap_or("").trim();
            if key.trim() == name && !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// 双提交 CSRF 校验（Cookie 值 vs 表单值，常数时间比较）
pub fn csrf_matches(cookie_value: Option<&str>, form_value: Option<&str>) -> bool {
    match (cookie_value, form_value) {
        (Some(left), Some(right)) => {
            !left.is_empty() && utils::oauth::constant_time_eq(left, right)
        }
        _ => false,
    }
}

/// 解析客户端 IP
///
/// 只有在显式设置 `TRUST_PROXY_HEADERS=true`（部署在反向代理之后）时才信任
/// `X-Forwarded-For`；否则一律用 TCP 连接对端地址，避免伪造头绕过 IP 白名单。
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    let trust_proxy = matches!(
        std::env::var("TRUST_PROXY_HEADERS")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    );
    if trust_proxy {
        if let Some(raw) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            // 取最左侧（最初的客户端）可解析地址
            for candidate in raw.split(',') {
                if let Ok(ip) = candidate.trim().parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
        if let Some(raw) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = raw.trim().parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    peer.map(|addr| addr.ip())
}

/// 表单 / 查询串的百分号解码（`+` 视为空格，非法 UTF-8 用替换字符兜底）
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 解析 `application/x-www-form-urlencoded` 请求体（重复键取第一个）
pub fn parse_form_body(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = percent_decode(parts.next().unwrap_or(""));
        let value = percent_decode(parts.next().unwrap_or(""));
        if !key.is_empty() {
            map.entry(key).or_insert(value);
        }
    }
    map
}

/// HTML 转义（同意页所有动态内容都必须经过这里）
pub fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// 同意页需要回传的授权参数（顺序固定，渲染时按此顺序输出隐藏域）
pub const CONSENT_FORM_FIELDS: [&str; 7] = [
    "response_type",
    "client_id",
    "redirect_uri",
    "scope",
    "state",
    "code_challenge",
    "code_challenge_method",
];

/// 渲染授权同意页
///
/// - 所有插值（客户端名称、scope、参数）都经过 [`html_escape`]；
/// - 表单 `method="post"` 提交回 `/oauth/authorize`，`decision` 区分同意 / 拒绝；
/// - 页面无外链、无内联脚本，配合 CSP `default-src 'none'` 也能正常显示。
pub fn render_consent_page(
    client_name: &str,
    client_id: &str,
    scopes: &[String],
    params: &HashMap<String, String>,
    csrf_token: &str,
) -> String {
    let mut hidden = String::new();
    for field in CONSENT_FORM_FIELDS {
        if let Some(value) = params.get(field) {
            hidden.push_str(&format!(
                r#"      <input type="hidden" name="{}" value="{}" />
"#,
                field,
                html_escape(value)
            ));
        }
    }
    hidden.push_str(&format!(
        r#"      <input type="hidden" name="csrf_token" value="{}" />
"#,
        html_escape(csrf_token)
    ));

    let scope_items = if scopes.is_empty() {
        "      <li>默认访问权限</li>\n".to_string()
    } else {
        scopes
            .iter()
            .map(|scope| format!("      <li><code>{}</code></li>\n", html_escape(scope)))
            .collect()
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>授权确认</title>
  <style>
    :root {{ color-scheme: light dark; }}
    body {{ font-family: system-ui, -apple-system, "Segoe UI", sans-serif; margin: 0;
           min-height: 100vh; display: flex; align-items: center; justify-content: center;
           background: #f5f6f8; color: #1f2328; }}
    main {{ width: min(440px, 92vw); background: #fff; border-radius: 12px; padding: 28px;
           box-shadow: 0 8px 30px rgba(0, 0, 0, .08); }}
    h1 {{ font-size: 20px; margin: 0 0 12px; }}
    p {{ line-height: 1.6; margin: 0 0 14px; }}
    ul {{ margin: 0 0 18px; padding-left: 20px; }}
    code {{ background: #f0f1f3; padding: 1px 5px; border-radius: 4px; font-size: 13px; }}
    .actions {{ display: flex; gap: 10px; margin: 0; }}
    button {{ flex: 1; padding: 10px 14px; border-radius: 8px; font-size: 14px; cursor: pointer;
             border: 1px solid #d0d7de; background: #fff; }}
    button.primary {{ background: #1f6feb; border-color: #1f6feb; color: #fff; }}
    .hint {{ font-size: 12px; color: #6e7781; margin: 14px 0 0; }}
  </style>
</head>
<body>
  <main>
    <h1>授权确认</h1>
    <p>应用 <strong>{client_name}</strong> 请求访问你的账号。</p>
    <p>客户端 ID：<code>{client_id}</code></p>
    <p>申请的权限范围：</p>
    <ul>
{scope_items}    </ul>
    <form method="post" action="/oauth/authorize">
{hidden}      <div class="actions">
        <button type="submit" name="decision" value="approve" class="primary">同意授权</button>
        <button type="submit" name="decision" value="deny">拒绝</button>
      </div>
    </form>
    <p class="hint">授权后可在「个人中心 → 授权管理」随时撤销。</p>
  </main>
</body>
</html>
"#,
        client_name = html_escape(client_name),
        client_id = html_escape(client_id),
        scope_items = scope_items,
        hidden = hidden,
    )
}

/// 渲染极简错误页（授权流程中的错误不返回 JSON，避免浏览器显示裸 JSON）
pub fn render_error_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title}</title>
  <style>
    body {{ font-family: system-ui, -apple-system, "Segoe UI", sans-serif; margin: 0;
           min-height: 100vh; display: flex; align-items: center; justify-content: center;
           background: #f5f6f8; color: #1f2328; }}
    main {{ width: min(420px, 92vw); background: #fff; border-radius: 12px; padding: 28px;
           box-shadow: 0 8px 30px rgba(0, 0, 0, .08); text-align: center; }}
    h1 {{ font-size: 18px; margin: 0 0 10px; }}
    p {{ line-height: 1.6; color: #57606a; margin: 0; }}
  </style>
</head>
<body>
  <main>
    <h1>{title}</h1>
    <p>{message}</p>
  </main>
</body>
</html>
"#,
        title = html_escape(title),
        message = html_escape(message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with_cookie(raw: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(raw).unwrap());
        headers
    }

    #[test]
    fn read_cookie_extracts_only_exact_name() {
        let headers = headers_with_cookie("theme=dark; ap_session=ss_abc; ap_csrf=cs_def");
        assert_eq!(
            read_cookie(&headers, SESSION_COOKIE_NAME),
            Some("ss_abc".to_string())
        );
        assert_eq!(
            read_cookie(&headers, CSRF_COOKIE_NAME),
            Some("cs_def".to_string())
        );
        // 前缀相同但名字不同，不能误匹配
        assert_eq!(read_cookie(&headers, "ap"), None);
        assert_eq!(read_cookie(&headers, "missing"), None);
        // 值为空视作未携带
        let empty = headers_with_cookie("ap_session=");
        assert_eq!(read_cookie(&empty, SESSION_COOKIE_NAME), None);
    }

    #[test]
    fn session_cookie_always_http_only_and_lax() {
        let cookie = build_session_cookie("ss_token", 3600);
        assert!(cookie.starts_with("ap_session=ss_token;"));
        assert!(cookie.contains("Path=/;"));
        assert!(cookie.contains("Max-Age=3600;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        // 负数 Max-Age 归一为 0（不允许意外延长）
        assert!(build_session_cookie("ss_token", -5).contains("Max-Age=0;"));

        // CSRF Cookie 必须可被同源脚本读取：不能带 HttpOnly
        let csrf = build_csrf_cookie("cs_token", 3600);
        assert!(csrf.starts_with("ap_csrf=cs_token;"));
        assert!(!csrf.contains("HttpOnly"));
        assert!(csrf.contains("SameSite=Lax"));
    }

    #[test]
    fn csrf_matches_requires_identical_non_empty_values() {
        assert!(csrf_matches(Some("cs_1"), Some("cs_1")));
        assert!(!csrf_matches(Some("cs_1"), Some("cs_2")));
        assert!(!csrf_matches(None, Some("cs_1")));
        assert!(!csrf_matches(Some("cs_1"), None));
        assert!(!csrf_matches(Some(""), Some("")));
    }

    #[test]
    fn percent_decode_handles_plus_percent_and_bad_sequences() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("http%3A%2F%2Flocalhost%3A5173%2Fcb"), "http://localhost:5173/cb");
        // 非法序列原样保留，不 panic
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        // 中文（UTF-8 多字节）
        assert_eq!(percent_decode("%E6%8E%88%E6%9D%83"), "授权");
    }

    #[test]
    fn parse_form_body_keeps_first_duplicate_and_skips_blank_keys() {
        let form = parse_form_body("decision=approve&state=xyz&scope=a+b&scope=dup&=empty&");
        assert_eq!(form.get("decision").map(String::as_str), Some("approve"));
        assert_eq!(form.get("state").map(String::as_str), Some("xyz"));
        assert_eq!(form.get("scope").map(String::as_str), Some("a b"));
        assert!(!form.contains_key(""));
    }

    #[test]
    fn html_escape_neutralises_markup() {
        assert_eq!(
            html_escape(r#"<script>alert("x")</script>"#),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a&b'c"), "a&amp;b&#39;c");
        assert_eq!(html_escape("普通文本"), "普通文本");
    }

    #[test]
    fn consent_page_escapes_client_supplied_values() {
        let mut params = HashMap::new();
        params.insert("client_id".to_string(), "app_1".to_string());
        params.insert(
            "redirect_uri".to_string(),
            "http://localhost:5173/cb\"><script>alert(1)</script>".to_string(),
        );
        let page = render_consent_page(
            "<img src=x onerror=alert(1)>",
            "app_1",
            &["read".to_string(), "<b>evil</b>".to_string()],
            &params,
            "cs_token",
        );
        // 危险字符全部转义，未闭合标签不得出现
        assert!(!page.contains("<img src=x"));
        assert!(!page.contains("<b>evil</b>"));
        assert!(!page.contains("<script>alert(1)</script>"));
        assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(page.contains("&lt;b&gt;evil&lt;/b&gt;"));
        // 表单骨架与 CSRF 隐藏域仍在
        assert!(page.contains(r#"<form method="post" action="/oauth/authorize">"#));
        assert!(page.contains(r#"name="csrf_token" value="cs_token""#));
        assert!(page.contains(r#"name="client_id" value="app_1""#));
        assert!(page.contains(r#"name="decision" value="approve""#));
    }

    #[test]
    fn error_page_escapes_arguments() {
        let page = render_error_page("<h1>x</h1>", "bad & wrong");
        assert!(page.contains("&lt;h1&gt;x&lt;/h1&gt;"));
        assert!(page.contains("bad &amp; wrong"));
    }
}
