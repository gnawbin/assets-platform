//! 纯解析 / SQL 片段构造
//!
//! 这里全是**不连库**的纯函数：把 `pg_catalog` 的原始值翻译成 `vector` crate
//! 的数据类型，或把请求翻译成 SQL 片段。拆出来是为了能在没有 PostgreSQL 的
//! 环境里跑单测（含畸形输入）。

use std::collections::BTreeMap;

use vector::{Distance, Filter, IndexKind, TableSpec, VectorError, VectorResult};

/// pgvector 的 `atttypmod` 换算维度：`typmod = 维度 + 4`
///
/// `atttypmod = -1` 表示列上没写维度（不定长 `vector`），此时返回 `None`。
pub fn dims_from_typmod(typmod: i32) -> Option<usize> {
    if typmod > 4 {
        usize::try_from(typmod - 4).ok()
    } else {
        None
    }
}

/// 列类型是否为 pgvector 的向量类型
///
/// 兼容 `vector(768)` / `halfvec(1024)` / `sparsevec` 这些写法。
pub fn is_vector_type(type_name: &str) -> bool {
    let base = type_name.trim().to_ascii_lowercase();
    let base = base.split('(').next().unwrap_or("").trim();
    matches!(base, "vector" | "halfvec" | "sparsevec")
}

/// 由索引操作符类推断距离度量（`vector_cosine_ops` / `vector_l2_ops` / `vector_ip_ops`）
pub fn distance_from_ops(ops: &str) -> Option<Distance> {
    let normalized = ops.to_ascii_lowercase();
    if normalized.contains("cosine") {
        Some(Distance::Cosine)
    } else if normalized.contains("_l2") {
        Some(Distance::L2)
    } else if normalized.contains("_ip") || normalized.contains("inner") {
        Some(Distance::InnerProduct)
    } else {
        None
    }
}

/// 由 `pg_am.amname` 推断索引类型
pub fn index_kind_from_access_method(access_method: &str) -> Option<IndexKind> {
    match access_method.trim().to_ascii_lowercase().as_str() {
        "hnsw" => Some(IndexKind::Hnsw),
        "ivfflat" => Some(IndexKind::IvfFlat),
        "flat" | "bruteforce" => Some(IndexKind::Flat),
        _ => None,
    }
}

/// 解析 `pg_get_indexdef()` 里的 `WITH (m='16', ef_construction='64')`
///
/// 畸形输入（缺括号、`=` 两边为空、多余逗号）一律跳过，绝不 panic。
pub fn index_params_from_ddl(ddl: &str) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    let lower = ddl.to_ascii_lowercase();
    let Some(with_at) = lower.find("with") else {
        return params;
    };
    let rest = &ddl[with_at + 4..];
    let Some(open) = rest.find('(') else {
        return params;
    };
    let Some(close) = rest[open..].find(')').map(|offset| open + offset) else {
        return params;
    };
    for part in rest[open + 1..close].split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_ascii_lowercase();
        let value = value.trim().trim_matches('\'').trim_matches('"');
        if key.is_empty() || value.is_empty() {
            continue;
        }
        params.insert(key, value.to_string());
    }
    params
}

/// 用双引号包裹标识符（内部 `"` 转义为 `""`）
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 拆分 `schema.table`
pub fn split_qualified(table: &str) -> Option<(&str, &str)> {
    table.split_once('.')
}

/// 把索引参数渲染成 ` WITH (k = v, ...)`
///
/// 纯数字值直接内联（`m = 16`），其它值加单引号并转义，避免拼接注入。
pub fn with_clause(parameters: &BTreeMap<String, String>) -> String {
    if parameters.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = parameters
        .iter()
        .map(|(key, value)| {
            if value.parse::<u32>().is_ok() {
                format!("{key} = {value}")
            } else {
                format!("{key} = '{}'", value.replace('\'', "''"))
            }
        })
        .collect();
    format!(" WITH ({})", pairs.join(", "))
}

/// 把过滤条件下推成 SQL 片段
///
/// 条件值一律作为**文本参数**绑定（序号从 `params.len() + 1` 开始），
/// 字段名只能是标识符（`Filter::validate` 已校验），因此可以安全内联。
pub fn build_filter_sql(
    filter: &Filter,
    params: &mut Vec<String>,
    metadata_column: &str,
) -> VectorResult<String> {
    filter.validate()?;
    match filter {
        Filter::Eq { field, value } => {
            params.push(value.clone());
            Ok(format!("{metadata_column}->>'{field}' = ${}", params.len()))
        }
        Filter::In { field, values } => {
            let mut slots = Vec::with_capacity(values.len());
            for value in values {
                params.push(value.clone());
                slots.push(format!("${}", params.len()));
            }
            Ok(format!(
                "{metadata_column}->>'{field}' IN ({})",
                slots.join(", ")
            ))
        }
        Filter::And(items) => Ok(format!(
            "({})",
            join(items, params, metadata_column, " AND ")?
        )),
        Filter::Or(items) => Ok(format!(
            "({})",
            join(items, params, metadata_column, " OR ")?
        )),
    }
}

fn join(
    items: &[Filter],
    params: &mut Vec<String>,
    metadata_column: &str,
    separator: &str,
) -> VectorResult<String> {
    let mut parts = Vec::with_capacity(items.len());
    for item in items {
        parts.push(build_filter_sql(item, params, metadata_column)?);
    }
    Ok(parts.join(separator))
}

/// 构造批量 upsert 语句（`ON CONFLICT (id) DO UPDATE`）
///
/// 每行 4 个（无文本列时 3 个）绑定参数：向量以文本绑定后 `::vector` 转型，
/// 元数据以文本绑定后 `::jsonb` 转型——这样不必给 sqlx 打开 `json` feature。
pub fn build_upsert_sql(qualified: &str, spec: &TableSpec, rows: usize) -> String {
    debug_assert!(rows > 0, "空批次不应进入 SQL 构造");
    let has_text = !spec.text_column.is_empty();
    let per_row = if has_text { 4 } else { 3 };
    let mut values = String::new();
    for row in 0..rows {
        if row > 0 {
            values.push(',');
        }
        let base = row * per_row;
        values.push_str(&format!("(${}, ${}::vector", base + 1, base + 2));
        if has_text {
            values.push_str(&format!(", ${}", base + 3));
        }
        values.push_str(&format!(", ${}::jsonb)", base + per_row));
    }
    let vector = &spec.vector_column;
    let metadata = &spec.metadata_column;
    let (columns, updates) = if has_text {
        let text = &spec.text_column;
        (
            format!("id, {vector}, {text}, {metadata}"),
            format!(
                "{vector} = EXCLUDED.{vector}, {text} = EXCLUDED.{text}, {metadata} = EXCLUDED.{metadata}"
            ),
        )
    } else {
        (
            format!("id, {vector}, {metadata}"),
            format!("{vector} = EXCLUDED.{vector}, {metadata} = EXCLUDED.{metadata}"),
        )
    };
    format!(
        "INSERT INTO {qualified} ({columns}) VALUES {values} ON CONFLICT (id) DO UPDATE SET {updates}"
    )
}

/// 生成可安全拼进 SQL 的 `"schema"."table"`
pub fn qualified_table(schema: &str, table: &str) -> VectorResult<String> {
    let (resolved_schema, name) = split_qualified(table).map_or((schema, table), |parts| parts);
    let mut statement = String::new();
    for part in [resolved_schema, name] {
        if !vector::is_identifier(part) {
            return Err(VectorError::config(format!("表名 / schema 非法：{part}")));
        }
        statement.push_str(&quote_ident(part));
        statement.push('.');
    }
    statement.pop();
    Ok(statement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typmod_maps_to_dimensions() {
        assert_eq!(dims_from_typmod(772), Some(768));
        assert_eq!(dims_from_typmod(5), Some(1));
        assert_eq!(dims_from_typmod(4), None);
        assert_eq!(dims_from_typmod(0), None);
        assert_eq!(dims_from_typmod(-1), None);
    }

    #[test]
    fn vector_type_detection() {
        assert!(is_vector_type("vector"));
        assert!(is_vector_type("vector(768)"));
        assert!(is_vector_type("VECTOR"));
        assert!(is_vector_type("halfvec(1024)"));
        assert!(is_vector_type("sparsevec"));
        assert!(is_vector_type(" vector "));
        assert!(!is_vector_type("text"));
        assert!(!is_vector_type("jsonb"));
        assert!(!is_vector_type("myvector"));
        assert!(!is_vector_type(""));
    }

    #[test]
    fn ops_and_access_method_mapping() {
        assert_eq!(
            distance_from_ops("vector_cosine_ops"),
            Some(Distance::Cosine)
        );
        assert_eq!(distance_from_ops("vector_l2_ops"), Some(Distance::L2));
        assert_eq!(
            distance_from_ops("halfvec_ip_ops"),
            Some(Distance::InnerProduct)
        );
        assert_eq!(distance_from_ops("btree"), None);
        assert_eq!(index_kind_from_access_method("hnsw"), Some(IndexKind::Hnsw));
        assert_eq!(
            index_kind_from_access_method(" IVFFLAT "),
            Some(IndexKind::IvfFlat)
        );
        assert_eq!(index_kind_from_access_method("btree"), None);
    }

    #[test]
    fn index_ddl_params_are_parsed_and_malformed_survive() {
        let ddl = "CREATE INDEX chunks_embedding_idx ON public.chunks USING hnsw \
                   (embedding vector_cosine_ops) WITH (m='16', ef_construction='64')";
        let params = index_params_from_ddl(ddl);
        assert_eq!(params.get("m").map(String::as_str), Some("16"));
        assert_eq!(
            params.get("ef_construction").map(String::as_str),
            Some("64")
        );

        // ivfflat 的 lists 是数字字面量
        let params =
            index_params_from_ddl("... USING ivfflat (embedding vector_l2_ops) WITH (lists=100)");
        assert_eq!(params.get("lists").map(String::as_str), Some("100"));

        // 畸形 / 无 WITH
        assert!(index_params_from_ddl("CREATE INDEX i ON t USING hnsw (v)").is_empty());
        assert!(index_params_from_ddl("WITH (").is_empty());
        assert!(index_params_from_ddl("WITH ()").is_empty());
        assert!(index_params_from_ddl("WITH (m)").is_empty());
        assert!(index_params_from_ddl("WITH (=16)").is_empty());
        assert!(index_params_from_ddl("").is_empty());
        assert_eq!(index_params_from_ddl("with (m=16, , ef='64')").len(), 2);
    }

    #[test]
    fn with_clause_quotes_non_numeric_values() {
        let mut params = BTreeMap::new();
        assert_eq!(with_clause(&params), "");
        params.insert("m".to_string(), "16".to_string());
        params.insert("ef_construction".to_string(), "64".to_string());
        assert_eq!(with_clause(&params), " WITH (ef_construction = 64, m = 16)");
        params.insert("note".to_string(), "it's".to_string());
        assert_eq!(
            with_clause(&params),
            " WITH (ef_construction = 64, m = 16, note = 'it''s')"
        );
    }

    #[test]
    fn filter_sql_uses_sequential_placeholders() {
        let mut params = Vec::new();
        let sql = build_filter_sql(&Filter::eq("lang", "zh"), &mut params, "metadata").unwrap();
        assert_eq!(sql, "metadata->>'lang' = $1");
        assert_eq!(params, vec!["zh".to_string()]);

        let mut params = Vec::new();
        let sql = build_filter_sql(
            &Filter::and(vec![
                Filter::eq("lang", "zh"),
                Filter::any_of("team", vec!["a".to_string(), "b".to_string()]),
            ]),
            &mut params,
            "metadata",
        )
        .unwrap();
        assert_eq!(
            sql,
            "(metadata->>'lang' = $1 AND metadata->>'team' IN ($2, $3))"
        );
        assert_eq!(params.len(), 3);

        // 非法字段名直接拒绝，且不产生任何参数
        let mut params = Vec::new();
        assert!(build_filter_sql(&Filter::eq("bad field", "v"), &mut params, "metadata").is_err());
        assert!(params.is_empty());
    }

    #[test]
    fn upsert_sql_shape_with_and_without_text() {
        let spec = TableSpec::new("chunks", 3);
        let sql = build_upsert_sql("\"public\".\"chunks\"", &spec, 2);
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"chunks\" (id, embedding, content, metadata) \
             VALUES ($1, $2::vector, $3, $4::jsonb),($5, $6::vector, $7, $8::jsonb) \
             ON CONFLICT (id) DO UPDATE SET embedding = EXCLUDED.embedding, \
             content = EXCLUDED.content, metadata = EXCLUDED.metadata"
        );

        let spec = spec.with_text_column("");
        let sql = build_upsert_sql("\"public\".\"chunks\"", &spec, 1);
        assert!(sql.contains("(id, embedding, metadata)"), "{sql}");
        assert!(sql.contains("VALUES ($1, $2::vector, $3::jsonb)"), "{sql}");
        assert!(!sql.contains("content"), "{sql}");
    }

    #[test]
    fn qualified_table_quotes_and_validates() {
        assert_eq!(
            qualified_table("public", "chunks").unwrap(),
            "\"public\".\"chunks\""
        );
        assert_eq!(
            qualified_table("public", "ops.chunks").unwrap(),
            "\"ops\".\"chunks\""
        );
        assert!(qualified_table("public", "chunks; drop table x").is_err());
        assert!(qualified_table("pub lic", "chunks").is_err());
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }
}
