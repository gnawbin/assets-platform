//! OAuth 2.1 传输与网络护栏（设计文档 §8.2 / §16 T-7）
//!
//! 三件事，全部**缺省安全**：
//!
//! 1. [`trusted_cidr_guard`]：`OAUTH_TRUSTED_CIDRS` 非空时只允许清单内来源访问
//!    `/oauth/*`（未配置 = 不限制，保持开发便利）；
//! 2. [`security_headers`]：统一补 `X-Content-Type-Options`、`X-Frame-Options`、
//!    `Referrer-Policy`，HTTPS 下再加 `Strict-Transport-Security`；
//! 3. [`hsts_enabled`] / [`cidr_list_from_env`]：纯函数，便于单测与文档说明。
//!
//! 判定所需的客户端 IP 来源见 [`client_ip_from`]：只有显式开启
//! `TRUST_PROXY_HEADERS` 才信任 `X-Forwarded-For`，否则用 TCP 对端地址，
//! 避免伪造请求头绕过白名单。

use std::net::IpAddr;

use axum::{
    body::Body,
    http::{header::HeaderName, HeaderValue, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

/// 一条 CIDR 规则（IPv4 / IPv6 通用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// 解析 `10.0.0.0/8`、`::1/128`、`192.168.1.5`（无 `/` 视作单主机）等写法
    pub fn parse(rule: &str) -> Option<Self> {
        let rule = rule.trim();
        if rule.is_empty() {
            return None;
        }
        let (address, prefix) = match rule.split_once('/') {
            Some((address, prefix)) => (address.trim(), Some(prefix.trim())),
            None => (rule, None),
        };
        let network: IpAddr = address.parse().ok()?;
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix {
            Some(raw) => {
                let value: u8 = raw.parse().ok()?;
                if value > max_prefix {
                    return None;
                }
                value
            }
            None => max_prefix,
        };
        Some(Self { network, prefix })
    }

    /// 判断地址是否落在本网段内
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, ip) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = prefix_mask_v4(self.prefix);
                (u32::from(network) & mask) == (u32::from(candidate) & mask)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = prefix_mask_v6(self.prefix);
                (u128::from(network) & mask) == (u128::from(candidate) & mask)
            }
            // IPv4 与 IPv6 不做隐式映射，避免 `::ffff:127.0.0.1` 绕过 v4 规则
            _ => false,
        }
    }
}

fn prefix_mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn prefix_mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

/// 解析逗号 / 分号 / 换行分隔的 CIDR 清单（非法条目忽略并告警）
pub fn parse_cidr_list(raw: &str) -> Vec<Cidr> {
    raw.split([',', ';', '\n'])
        .filter(|item| !item.trim().is_empty())
        .filter_map(|item| {
            let parsed = Cidr::parse(item);
            if parsed.is_none() {
                tracing::warn!(rule = %item.trim(), "忽略非法的 CIDR 规则");
            }
            parsed
        })
        .collect()
}

/// 读取 `OAUTH_TRUSTED_CIDRS`（未配置 / 全非法 → 空，表示不限制）
pub fn cidr_list_from_env() -> Vec<Cidr> {
    std::env::var("OAUTH_TRUSTED_CIDRS")
        .map(|raw| parse_cidr_list(&raw))
        .unwrap_or_default()
}

/// 白名单判定：清单为空 = 不限制；有清单但拿不到 IP = 拒绝（fail-closed）
pub fn ip_allowed(cidrs: &[Cidr], ip: Option<IpAddr>) -> bool {
    if cidrs.is_empty() {
        return true;
    }
    match ip {
        Some(ip) => cidrs.iter().any(|cidr| cidr.contains(ip)),
        None => false,
    }
}

/// 是否信任 `X-Forwarded-For`（只有反向代理之后才应开启）
pub fn trust_proxy_headers() -> bool {
    matches!(
        std::env::var("TRUST_PROXY_HEADERS")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

/// 从请求扩展 / 头里取客户端 IP
///
/// `peer` 由 `axum::extract::ConnectInfo<SocketAddr>` 提供（HTTP 服务启动时启用）；
/// 反向代理场景下优先使用受信的 `X-Forwarded-For` 最左值。
pub fn client_ip_from(headers: &axum::http::HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
    if trust_proxy_headers() {
        if let Some(raw) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            for candidate in raw.split(',') {
                if let Ok(ip) = candidate.trim().parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
        if let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }
    peer
}

/// `/oauth/*` 的来源白名单中间件（`OAUTH_TRUSTED_CIDRS` 为空时直接放行）
pub async fn trusted_cidr_guard(req: Request<Body>, next: Next) -> Response {
    let cidrs = cidr_list_from_env();
    if cidrs.is_empty() {
        return next.run(req).await;
    }

    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip());
    let ip = client_ip_from(req.headers(), peer);

    if ip_allowed(&cidrs, ip) {
        return next.run(req).await;
    }

    tracing::warn!(ip = ?ip, route = %req.uri().path(), "OAuth 端点来源不在 OAUTH_TRUSTED_CIDRS 内，拒绝");
    let body = Json(serde_json::json!({
        "error": "access_denied",
        "error_description": "来源地址不在允许的网段内"
    }));
    let mut response = (StatusCode::FORBIDDEN, body).into_response();
    response
        .headers_mut()
        .insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    response
}

/// HSTS 开关（默认开启；显式 `HSTS_ENABLED=false` 可关闭，仅限本地调试）
pub fn hsts_enabled() -> bool {
    !matches!(
        std::env::var("HSTS_ENABLED")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0") | Ok("false") | Ok("no") | Ok("off")
    )
}

/// 计算本次响应应附带的固定安全头
///
/// 纯函数：`https` 决定是否加 HSTS，`hsts_switch` 是运维开关。
pub fn security_header_pairs(https: bool, hsts_switch: bool) -> Vec<(HeaderName, HeaderValue)> {
    let mut pairs = vec![
        (
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ),
        (
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ),
        (
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ),
    ];
    if https && hsts_switch {
        pairs.push((
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));
    }
    pairs
}

/// 请求是否经由 HTTPS 到达（`X-Forwarded-Proto` 只在信任代理时采信）
pub fn is_https_request(headers: &axum::http::HeaderMap) -> bool {
    if trust_proxy_headers() {
        if let Some(proto) = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
        {
            return proto
                .split(',')
                .next()
                .map(|value| value.trim().eq_ignore_ascii_case("https"))
                .unwrap_or(false);
        }
    }
    // 未经过代理：以 issuer 的 scheme 作为部署事实（TLS 通常由前置代理终止）
    std::env::var("OAUTH_ISSUER")
        .map(|issuer| issuer.trim().to_ascii_lowercase().starts_with("https://"))
        .unwrap_or(false)
}

/// 全局安全响应头中间件
pub async fn security_headers(req: Request<Body>, next: Next) -> Response {
    let https = is_https_request(req.headers());
    let pairs = security_header_pairs(https, hsts_enabled());
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in pairs {
        headers.insert(name, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("测试用 IP 必须合法")
    }

    #[test]
    fn cidr_matches_ipv4_ranges() {
        let cidr = Cidr::parse("10.0.0.0/8").expect("合法 CIDR");
        assert!(cidr.contains(ip("10.1.2.3")));
        assert!(cidr.contains(ip("10.255.255.255")));
        assert!(!cidr.contains(ip("11.0.0.1")));
        assert!(!cidr.contains(ip("192.168.1.1")));

        // 单主机写法（无 /）
        let single = Cidr::parse("192.168.1.5").expect("合法主机规则");
        assert!(single.contains(ip("192.168.1.5")));
        assert!(!single.contains(ip("192.168.1.6")));

        // /0 匹配全部 v4
        let all = Cidr::parse("0.0.0.0/0").expect("合法 CIDR");
        assert!(all.contains(ip("8.8.8.8")));
        assert!(!all.contains(ip("::1")));
    }

    #[test]
    fn cidr_matches_ipv6_ranges() {
        let cidr = Cidr::parse("2001:db8::/32").expect("合法 CIDR");
        assert!(cidr.contains(ip("2001:db8:1234::1")));
        assert!(!cidr.contains(ip("2001:db9::1")));

        let loopback = Cidr::parse("::1/128").expect("合法 CIDR");
        assert!(loopback.contains(ip("::1")));
        assert!(!loopback.contains(ip("127.0.0.1")));

        // IPv4-mapped 地址不得命中 v4 规则（避免绕过）
        let v4 = Cidr::parse("127.0.0.0/8").expect("合法 CIDR");
        assert!(!v4.contains(ip("::ffff:127.0.0.1")));
    }

    #[test]
    fn cidr_rejects_malformed_rules() {
        assert!(Cidr::parse("").is_none());
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("::1/129").is_none());
        assert!(Cidr::parse("not-an-ip/8").is_none());
        assert!(Cidr::parse("10.0.0.0/abc").is_none());
    }

    #[test]
    fn cidr_list_skips_invalid_entries() {
        let list = parse_cidr_list(" 10.0.0.0/8 , 192.168.0.0/16 ;bogus\n::1/128,");
        assert_eq!(list.len(), 3);
        assert!(list[0].contains(ip("10.9.9.9")));
        assert!(list[1].contains(ip("192.168.5.5")));
        assert!(list[2].contains(ip("::1")));
        // 全是非法内容 → 空清单（等同于不限制）
        assert!(parse_cidr_list("bogus, still-bogus").is_empty());
    }

    #[test]
    fn ip_allowed_is_fail_closed_when_list_configured() {
        let cidrs = parse_cidr_list("10.0.0.0/8");
        // 未配置清单：不限制（含拿不到 IP 的情况）
        assert!(ip_allowed(&[], None));
        assert!(ip_allowed(&[], Some(ip("203.0.113.9"))));
        // 配置了清单：命中放行
        assert!(ip_allowed(&cidrs, Some(ip("10.1.1.1"))));
        // 配置了清单但不匹配 / 无法确定来源：拒绝
        assert!(!ip_allowed(&cidrs, Some(ip("203.0.113.9"))));
        assert!(!ip_allowed(&cidrs, None));
    }

    #[test]
    fn security_headers_include_hsts_only_for_https() {
        let names = |https: bool, hsts: bool| {
            security_header_pairs(https, hsts)
                .into_iter()
                .map(|(name, _)| name.as_str().to_string())
                .collect::<Vec<_>>()
        };

        let base = names(false, true);
        assert!(base.contains(&"x-content-type-options".to_string()));
        assert!(base.contains(&"x-frame-options".to_string()));
        assert!(base.contains(&"referrer-policy".to_string()));
        assert!(!base.contains(&"strict-transport-security".to_string()));

        let https = names(true, true);
        assert!(https.contains(&"strict-transport-security".to_string()));

        // 运维关闭 HSTS 后即使 HTTPS 也不下发
        let disabled = names(true, false);
        assert!(!disabled.contains(&"strict-transport-security".to_string()));
    }

    #[test]
    fn security_header_values_are_expected() {
        let pairs = security_header_pairs(true, true);
        let find = |want: &str| {
            pairs
                .iter()
                .find(|(name, _)| name.as_str() == want)
                .map(|(_, value)| value.to_str().unwrap().to_string())
        };
        assert_eq!(find("x-content-type-options").as_deref(), Some("nosniff"));
        assert_eq!(find("x-frame-options").as_deref(), Some("DENY"));
        assert_eq!(find("referrer-policy").as_deref(), Some("no-referrer"));
        assert_eq!(
            find("strict-transport-security").as_deref(),
            Some("max-age=31536000; includeSubDomains")
        );
    }
}

