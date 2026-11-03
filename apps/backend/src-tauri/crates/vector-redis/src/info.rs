//! `INFO` / `INFO keyspace` / `CLUSTER NODES` / `FT.INFO` 的纯解析
//!
//! 全部是纯函数：把服务端返回的文本（或已拆好的 `FT.INFO` 名值对）
//! 翻译成结构体，畸形输入一律跳过而不是 panic。这样 90% 的采集逻辑
//! 可以在没有 Redis 的环境里被单测覆盖。

use std::collections::BTreeMap;

/// `INFO` 的解析结果：`section -> field -> value`（key 统一小写）
pub type InfoSections = BTreeMap<String, BTreeMap<String, String>>;

/// 解析 `INFO [section]` 文本
///
/// `# Server` 这样的段头建立新段；段头之前的内容落到 `default` 段。
pub fn parse_info(text: &str) -> InfoSections {
    let mut sections: InfoSections = BTreeMap::new();
    let mut current = String::from("default");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('#') {
            current = name.trim().to_ascii_lowercase();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        sections
            .entry(current.clone())
            .or_default()
            .insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    sections
}

/// 便捷取值：`parse_info(...)` 后按段 / 字段取字符串
pub fn info_field<'a>(sections: &'a InfoSections, section: &str, field: &str) -> Option<&'a str> {
    sections
        .get(&section.to_ascii_lowercase())
        .and_then(|section| section.get(&field.to_ascii_lowercase()))
        .map(String::as_str)
}

/// `INFO keyspace` 里的一个库
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyspaceDb {
    /// 库名（`db0`、`db1` …）
    pub db: String,
    /// 键数量
    pub keys: u64,
    /// 带过期时间的键数量
    pub expires: u64,
    /// 平均 TTL（毫秒）
    pub avg_ttl: u64,
}

/// 解析 `INFO keyspace`
///
/// 形如 `db0:keys=12,expires=0,avg_ttl=0`；非法数字按 0 处理，
/// 因此半截 / 脏数据不会让整次采集失败。
pub fn parse_keyspace(text: &str) -> Vec<KeyspaceDb> {
    let mut databases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("db") {
            continue;
        }
        let Some((name, body)) = line.split_once(':') else {
            continue;
        };
        let mut keyspace = KeyspaceDb {
            db: name.trim().to_string(),
            keys: 0,
            expires: 0,
            avg_ttl: 0,
        };
        for part in body.split(',') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let number = value.trim().parse::<u64>().unwrap_or(0);
            match key.trim() {
                "keys" => keyspace.keys = number,
                "expires" => keyspace.expires = number,
                "avg_ttl" => keyspace.avg_ttl = number,
                _ => {}
            }
        }
        databases.push(keyspace);
    }
    databases
}

/// `CLUSTER NODES` 里的一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterNode {
    /// 节点 ID
    pub id: String,
    /// `ip:port@cport`
    pub address: String,
    /// 是否为 master（`slave` / `replica` 标记为 false）
    pub master: bool,
    /// 负责的槽位总数（区间按含端点计算）
    pub slots: u32,
}

/// 解析 `CLUSTER NODES` 文本
///
/// 每行至少 8 列：`id address flags master ping pong epoch link-state ...slots`。
/// 字段不足或槽位写坏时跳过该行 / 该槽位，不 panic。
pub fn parse_cluster_nodes(text: &str) -> Vec<ClusterNode> {
    let mut nodes = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let flags = fields[2];
        let mut slots = 0_u32;
        for token in fields.iter().skip(8) {
            // `[hostname]`、`[ip]` 这类附加信息与槽位无关
            if token.starts_with('[') || token.starts_with(']') {
                continue;
            }
            if let Some((start, end)) = token.split_once('-') {
                if let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) {
                    slots += end.saturating_sub(start) + 1;
                }
            } else if token.parse::<u32>().is_ok() {
                slots += 1;
            }
        }
        nodes.push(ClusterNode {
            id: fields[0].to_string(),
            address: fields[1].to_string(),
            master: !flags.contains("slave") && !flags.contains("replica"),
            slots,
        });
    }
    nodes
}

/// 把 `FT.INFO` 交替出现的名值对拍平成 map（key 统一小写）
///
/// `FT.INFO` 的回复是 `[name, value, name, value, …]`，其中 value 可能是
/// 嵌套数组；这里只保留标量，嵌套项直接跳过（采集只需要 `num_docs`、
/// `index_name`、`attributes` 之类的标量）。
pub fn parse_ft_info(pairs: &[(String, Option<String>)]) -> BTreeMap<String, String> {
    let mut info = BTreeMap::new();
    for (name, value) in pairs {
        let Some(value) = value else { continue };
        info.insert(name.trim().to_ascii_lowercase(), value.clone());
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO_TEXT: &str = "# Server\r\nredis_version:7.2.5\r\nos:Linux\r\n\r\n# Modules\r\n\
module:name=search,ver=20811,api=1\r\n\r\n# Keyspace\r\ndb0:keys=12,expires=3,avg_ttl=900\r\n";

    #[test]
    fn info_sections_are_split() {
        let sections = parse_info(INFO_TEXT);
        assert_eq!(
            info_field(&sections, "Server", "redis_version"),
            Some("7.2.5")
        );
        assert_eq!(info_field(&sections, "server", "os"), Some("Linux"));
        assert!(info_field(&sections, "keyspace", "db0").is_some());
        assert_eq!(info_field(&sections, "server", "missing"), None);
        assert_eq!(info_field(&sections, "nope", "os"), None);
    }

    #[test]
    fn info_without_sections_falls_back_to_default() {
        let sections = parse_info("redis_version:7.0.0\nbroken-line\n\n");
        assert_eq!(
            info_field(&sections, "default", "redis_version"),
            Some("7.0.0")
        );
        assert!(parse_info("").is_empty());
    }

    #[test]
    fn keyspace_lines_are_parsed() {
        let databases =
            parse_keyspace("# Keyspace\r\ndb0:keys=12,expires=3,avg_ttl=900\r\ndb1:keys=0\r\n");
        assert_eq!(databases.len(), 2);
        assert_eq!(databases[0].db, "db0");
        assert_eq!(databases[0].keys, 12);
        assert_eq!(databases[0].expires, 3);
        assert_eq!(databases[0].avg_ttl, 900);
        assert_eq!(databases[1].keys, 0);

        // 空 / 畸形输入
        assert!(parse_keyspace("").is_empty());
        assert!(parse_keyspace("db0").is_empty());
        let malformed = parse_keyspace("db2:keys=abc,expires=,oops");
        assert_eq!(malformed.len(), 1);
        assert_eq!(malformed[0].keys, 0);
        assert_eq!(malformed[0].expires, 0);
    }

    #[test]
    fn cluster_nodes_are_parsed() {
        let text = "\
07c37dfeb235213a872192d90877d0cd55635b91 127.0.0.1:30004@31004 myself,master - 0 0 2 connected 5461-10922\n\
second 127.0.0.1:30001@31001 master - 0 0 1 connected 0-5460 12288\n\
third 127.0.0.1:30002@31002 slave 07c3 0 0 2 connected\n\
broken line\n";
        let nodes = parse_cluster_nodes(text);
        assert_eq!(nodes.len(), 3);
        assert!(nodes[0].master);
        assert_eq!(nodes[0].slots, 5462);
        assert_eq!(nodes[1].slots, 5462);
        assert!(!nodes[2].master);
        assert_eq!(nodes[2].slots, 0);
        assert!(parse_cluster_nodes("").is_empty());
    }

    #[test]
    fn ft_info_pairs_are_flattened() {
        let pairs = vec![
            ("index_name".to_string(), Some("chunks_idx".to_string())),
            ("num_docs".to_string(), Some("42".to_string())),
            ("attributes".to_string(), None),
        ];
        let info = parse_ft_info(&pairs);
        assert_eq!(
            info.get("index_name").map(String::as_str),
            Some("chunks_idx")
        );
        assert_eq!(info.get("num_docs").map(String::as_str), Some("42"));
        assert!(!info.contains_key("attributes"));
    }
}
