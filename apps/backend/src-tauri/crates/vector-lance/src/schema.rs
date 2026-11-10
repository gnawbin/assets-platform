//! Arrow / LanceDB 数据类型与记录编解码
//!
//! 本模块只处理「[`vector`] 契约类型 ↔ Arrow 列」的翻译：
//! - 建表 schema：`id` + 原文列 + 元数据列 + 向量列（`FixedSizeList<Float32, dims>`）；
//! - 写入批：把 [`VectorRecord`] 打成 [`RecordBatch`]；
//! - 召回解码：把 `FT.SEARCH` 对应的 LanceDB 结果批解成 [`SearchHit`]；
//! - 过滤下推：把 [`Filter`] 翻成 SQL（只支持 `id`，元数据是 JSON 文本列）。

use std::sync::Arc;

use lancedb::DistanceType;
use lancedb::arrow::arrow_array::{
    Array, ArrayRef, AsArray, FixedSizeListArray, Float32Array, Float32Type, RecordBatch,
    RecordBatchIterator, RecordBatchReader, StringArray,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema, SchemaRef};
use lancedb::index::IndexType;
use vector::{
    Distance, Filter, IndexKind, SearchHit, TableSpec, VectorError, VectorRecord, VectorResult,
};

/// 主键列名（LanceDB 没有主键约束，`merge_insert` 靠它对齐）
pub const ID_COLUMN: &str = "id";
/// LanceDB 返回的距离列名
pub const DISTANCE_COLUMN: &str = "_distance";
/// `FixedSizeList` 的子字段名（arrow 的约定）
const ITEM_FIELD: &str = "item";

/// 契约距离度量 → LanceDB 距离类型
pub fn distance_type(distance: Distance) -> DistanceType {
    match distance {
        Distance::Cosine => DistanceType::Cosine,
        Distance::L2 => DistanceType::L2,
        // 内积在 Lance 里叫 Dot（距离 = -dot，越小越相近）
        Distance::InnerProduct => DistanceType::Dot,
    }
}

/// LanceDB 距离类型 → 契约距离度量；无法对应（如 Hamming）返回 `None`
pub fn distance_from_type(metric: DistanceType) -> Option<Distance> {
    match metric {
        DistanceType::Cosine => Some(Distance::Cosine),
        DistanceType::L2 => Some(Distance::L2),
        DistanceType::Dot => Some(Distance::InnerProduct),
        _ => None,
    }
}

/// LanceDB 索引类型 → 契约索引类型；标量索引返回 `None`
pub fn index_kind(index_type: &IndexType) -> Option<IndexKind> {
    match index_type {
        IndexType::IvfFlat | IndexType::IvfSq | IndexType::IvfRq => Some(IndexKind::IvfFlat),
        IndexType::IvfPq => Some(IndexKind::IvfPq),
        IndexType::IvfHnswPq | IndexType::IvfHnswSq | IndexType::IvfHnswFlat => {
            Some(IndexKind::Hnsw)
        }
        _ => None,
    }
}

/// 向量列的 Arrow 类型：`FixedSizeList<Float32, dims>`
pub fn vector_data_type(dimensions: usize) -> VectorResult<DataType> {
    let size = i32::try_from(dimensions)
        .map_err(|_| VectorError::config(format!("维度超出 i32 上限：{dimensions}")))?;
    Ok(DataType::FixedSizeList(
        Arc::new(Field::new(ITEM_FIELD, DataType::Float32, true)),
        size,
    ))
}

/// 建表用的 schema（列顺序与 [`records_batch`] 严格一致）
pub fn table_schema(spec: &TableSpec) -> VectorResult<SchemaRef> {
    let mut fields = vec![Field::new(ID_COLUMN, DataType::Utf8, false)];
    if !spec.text_column.is_empty() {
        fields.push(Field::new(&spec.text_column, DataType::Utf8, true));
    }
    fields.push(Field::new(&spec.metadata_column, DataType::Utf8, true));
    fields.push(Field::new(
        &spec.vector_column,
        vector_data_type(spec.dimensions)?,
        true,
    ));
    Ok(Arc::new(Schema::new(fields)))
}

/// 把一批记录打成 [`RecordBatch`]
pub fn records_batch(spec: &TableSpec, records: &[VectorRecord]) -> VectorResult<RecordBatch> {
    let schema = table_schema(spec)?;
    let ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
    let mut columns: Vec<ArrayRef> = vec![Arc::new(StringArray::from(ids))];
    if !spec.text_column.is_empty() {
        let texts: Vec<Option<&str>> = records
            .iter()
            .map(|record| record.text.as_deref())
            .collect();
        columns.push(Arc::new(StringArray::from(texts)));
    }
    let metadata: Vec<String> = records
        .iter()
        .map(|record| {
            serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".to_string())
        })
        .collect();
    columns.push(Arc::new(StringArray::from(metadata)));
    columns.push(Arc::new(vector_array(spec, records)?));
    RecordBatch::try_new(schema, columns)
        .map_err(|error| VectorError::backend(format!("组装写入批失败：{error}")))
}

/// 写入用的批读取器（`merge_insert` / `add` 都接受它）
pub fn records_reader(
    spec: &TableSpec,
    records: &[VectorRecord],
) -> VectorResult<Box<dyn RecordBatchReader + Send>> {
    let schema = table_schema(spec)?;
    let batch = records_batch(spec, records)?;
    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

/// 记录里的向量 → `FixedSizeList<Float32, dims>`
fn vector_array(spec: &TableSpec, records: &[VectorRecord]) -> VectorResult<FixedSizeListArray> {
    let size = i32::try_from(spec.dimensions)
        .map_err(|_| VectorError::config(format!("维度超出 i32 上限：{}", spec.dimensions)))?;
    let mut flat: Vec<f32> = Vec::with_capacity(records.len() * spec.dimensions);
    for record in records {
        if record.vector.len() != spec.dimensions {
            return Err(VectorError::schema(format!(
                "记录 {} 的向量维度 {} 与表维度 {} 不一致",
                record.id,
                record.vector.len(),
                spec.dimensions
            )));
        }
        flat.extend_from_slice(&record.vector);
    }
    let item = Arc::new(Field::new(ITEM_FIELD, DataType::Float32, true));
    FixedSizeListArray::try_new(item, size, Arc::new(Float32Array::from(flat)), None)
        .map_err(|error| VectorError::backend(format!("构造向量列失败：{error}")))
}

/// 把 [`Filter`] 翻成 LanceDB 的 SQL 过滤条件
///
/// 只有 `id` 是独立列；元数据整体存成 JSON 文本列，无法按字段下推，
/// 因此对这些字段直接返回 `Unsupported`，而不是静默忽略。
pub fn filter_sql(filter: &Filter) -> VectorResult<String> {
    match filter {
        Filter::Eq { field, value } => {
            ensure_filter_field(field)?;
            Ok(format!("{ID_COLUMN} = {}", sql_literal(value)))
        }
        Filter::In { field, values } => {
            ensure_filter_field(field)?;
            if values.is_empty() {
                return Err(VectorError::config(format!(
                    "过滤字段 {field} 的 IN 候选值不能为空"
                )));
            }
            let list: Vec<String> = values.iter().map(|value| sql_literal(value)).collect();
            Ok(format!("{ID_COLUMN} IN ({})", list.join(", ")))
        }
        Filter::And(items) => combine(items, " AND "),
        Filter::Or(items) => combine(items, " OR "),
    }
}

/// 递归组合 AND / OR，并给每段加括号保证优先级
fn combine(items: &[Filter], glue: &str) -> VectorResult<String> {
    if items.is_empty() {
        return Err(VectorError::config("逻辑组合不能为空"));
    }
    let parts = items
        .iter()
        .map(|item| filter_sql(item).map(|sql| format!("({sql})")))
        .collect::<VectorResult<Vec<String>>>()?;
    Ok(parts.join(glue))
}

/// 只有 `id` 能下推
fn ensure_filter_field(field: &str) -> VectorResult<()> {
    if field == ID_COLUMN {
        return Ok(());
    }
    Err(VectorError::unsupported(format!(
        "LanceDB 副本只把 {ID_COLUMN} 建成独立列，元数据是 JSON 文本列，无法下推字段 {field} 的过滤"
    )))
}

/// SQL 字符串字面量（单引号翻倍转义）
fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 召回结果里要取的列（`vector` 为 `Some` 时一并回传向量）
#[derive(Debug, Clone, Copy)]
pub struct HitColumns<'a> {
    /// 原文列名（空字符串表示不取）
    pub text: &'a str,
    /// 元数据列名
    pub metadata: &'a str,
    /// 向量列名（`Some` 表示回传向量）
    pub vector: Option<&'a str>,
}

/// 把 LanceDB 的查询结果批解码成 [`SearchHit`]
pub fn hits_from_batches(
    batches: &[RecordBatch],
    columns: HitColumns<'_>,
    distance: Distance,
) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for batch in batches {
        let Some(ids) = batch
            .column_by_name(ID_COLUMN)
            .and_then(|array| array.as_string_opt::<i32>())
        else {
            continue;
        };
        let distances = batch.column_by_name(DISTANCE_COLUMN);
        let texts = batch
            .column_by_name(columns.text)
            .and_then(|array| array.as_string_opt::<i32>());
        let metadata = batch
            .column_by_name(columns.metadata)
            .and_then(|array| array.as_string_opt::<i32>());
        let vectors = columns.vector.and_then(|name| batch.column_by_name(name));
        for row in 0..batch.num_rows() {
            if ids.is_null(row) {
                continue;
            }
            let raw_distance = distances
                .and_then(|array| distance_at(array, row))
                .unwrap_or_default();
            let mut hit = SearchHit::from_distance(ids.value(row), raw_distance, distance);
            if let Some(values) = texts
                && !values.is_null(row)
            {
                hit.text = Some(values.value(row).to_string());
            }
            if let Some(values) = metadata
                && !values.is_null(row)
                && let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                    values.value(row),
                )
            {
                hit.metadata = map.into_iter().collect();
            }
            if let Some(array) = vectors {
                hit.vector = vector_at(array, row);
            }
            hits.push(hit);
        }
    }
    hits
}

/// 取距离：单向量查询是 `Float32`，多向量查询会包一层 `FixedSizeList`
fn distance_at(array: &ArrayRef, row: usize) -> Option<f32> {
    if let Some(values) = array.as_primitive_opt::<Float32Type>() {
        return (!values.is_null(row)).then(|| values.value(row));
    }
    let list = array.as_fixed_size_list_opt()?;
    if list.is_null(row) {
        return None;
    }
    let inner = list.value(row);
    inner
        .as_primitive_opt::<Float32Type>()
        .map(|values| values.value(0))
}

/// 取向量：`FixedSizeList<Float32>` → `Vec<f32>`
fn vector_at(array: &ArrayRef, row: usize) -> Option<Vec<f32>> {
    let list = array.as_fixed_size_list_opt()?;
    if list.is_null(row) {
        return None;
    }
    let inner = list.value(row);
    inner
        .as_primitive_opt::<Float32Type>()
        .map(|values| values.values().to_vec())
}

/// 从已有表的 schema 里找出向量列与维度
///
/// LanceDB 允许一张表有多个 `FixedSizeList<float>` 列；按列顺序取第一个，
/// 与 `describe` 的展示口径保持一致。
/// 从已有表的 schema 里找出向量列与维度
///
/// LanceDB 允许一张表有多个 `FixedSizeList<float>` 列；按列顺序取第一个，
/// 与 `describe` 的展示口径保持一致。
pub fn vector_layout(schema: &Schema) -> Option<(String, usize)> {
    for field in schema.fields() {
        if let DataType::FixedSizeList(item, size) = field.data_type()
            && *size > 0
            && matches!(
                item.data_type(),
                DataType::Float32 | DataType::Float16 | DataType::Float64
            )
        {
            return Some((field.name().clone(), *size as usize));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TableSpec {
        TableSpec::new("chunks", 3)
    }

    fn records() -> Vec<VectorRecord> {
        vec![
            VectorRecord::new("a", vec![0.1, 0.2, 0.3]).with_text("alpha"),
            VectorRecord::new("b", vec![0.4, 0.5, 0.6]).with_metadata("source", "wiki"),
        ]
    }

    /// 列数 = id + [原文] + 元数据 + 向量
    fn spec_column_count(spec: &TableSpec) -> usize {
        3 + usize::from(!spec.text_column.is_empty())
    }

    #[test]
    fn schema_exposes_vector_column() {
        let schema = table_schema(&spec()).unwrap();
        assert_eq!(schema.field(0).name(), ID_COLUMN);
        assert_eq!(vector_layout(&schema), Some(("embedding".to_string(), 3)));

        // 没有向量列的表
        let plain = Schema::new(vec![Field::new("id", DataType::Utf8, false)]);
        assert_eq!(vector_layout(&plain), None);
    }

    #[test]
    fn batches_match_schema_and_vectors() {
        let spec = spec();
        let batch = records_batch(&spec, &records()).unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), spec_column_count(&spec));
        assert_eq!(batch.schema(), table_schema(&spec).unwrap());

        // 向量列内容按行展开
        let vector = batch.column_by_name("embedding").unwrap();
        assert_eq!(vector_at(vector, 0), Some(vec![0.1, 0.2, 0.3]));
        assert_eq!(vector_at(vector, 1), Some(vec![0.4, 0.5, 0.6]));

        // 维度不一致直接拒绝
        let bad = vec![VectorRecord::new("c", vec![1.0, 2.0])];
        assert_eq!(records_batch(&spec, &bad).unwrap_err().kind(), "schema");
    }

    #[test]
    fn records_reader_is_usable_as_scannable() {
        let reader = records_reader(&spec(), &records()).unwrap();
        let batches: Vec<RecordBatch> = reader.map(|item| item.expect("批读取失败")).collect();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 2);
    }

    #[test]
    fn hits_decode_distance_text_metadata_and_vector() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(ID_COLUMN, DataType::Utf8, false),
            Field::new("content", DataType::Utf8, true),
            Field::new("metadata", DataType::Utf8, true),
            Field::new("embedding", vector_data_type(2).unwrap(), true),
            Field::new(DISTANCE_COLUMN, DataType::Float32, true),
        ]));
        let vectors = FixedSizeListArray::try_new(
            Arc::new(Field::new(ITEM_FIELD, DataType::Float32, true)),
            2,
            Arc::new(Float32Array::from(vec![0.1_f32, 0.2, 0.3, 0.4])),
            None,
        )
        .unwrap();
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["a", "b"])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("alpha"), None])),
                Arc::new(StringArray::from(vec![
                    Some(r#"{"source":"wiki"}"#),
                    Some("not-json"),
                ])),
                Arc::new(vectors),
                Arc::new(Float32Array::from(vec![Some(0.5_f32), None])),
            ],
        )
        .unwrap();

        let hits = hits_from_batches(
            &[batch],
            HitColumns {
                text: "content",
                metadata: "metadata",
                vector: Some("embedding"),
            },
            Distance::Cosine,
        );
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "a");
        assert_eq!(hits[0].distance, Some(0.5));
        // score = 1 - 余弦距离
        assert!((hits[0].score - 0.5).abs() < 1e-6);
        assert_eq!(hits[0].text.as_deref(), Some("alpha"));
        assert_eq!(
            hits[0].metadata.get("source"),
            Some(&serde_json::json!("wiki"))
        );
        assert_eq!(hits[0].vector, Some(vec![0.1, 0.2]));

        // 缺距离按 0 处理；非法 JSON 不算错
        assert_eq!(hits[1].distance, Some(0.0));
        assert!(hits[1].text.is_none());
        assert!(hits[1].metadata.is_empty());
        assert_eq!(hits[1].vector, Some(vec![0.3, 0.4]));
    }

    #[test]
    fn filter_sql_only_pushes_down_id() {
        assert_eq!(
            filter_sql(&Filter::eq("id", "doc-1")).unwrap(),
            "id = 'doc-1'"
        );
        // 单引号转义
        assert_eq!(
            filter_sql(&Filter::eq("id", "o'brien")).unwrap(),
            "id = 'o''brien'"
        );
        assert_eq!(
            filter_sql(&Filter::any_of("id", vec!["a".into(), "b".into()])).unwrap(),
            "id IN ('a', 'b')"
        );
        assert_eq!(
            filter_sql(&Filter::and(vec![
                Filter::eq("id", "a"),
                Filter::any_of("id", vec!["b".into()]),
            ]))
            .unwrap(),
            "(id = 'a') AND (id IN ('b'))"
        );

        // 元数据字段是 JSON 文本列，无法下推
        assert_eq!(
            filter_sql(&Filter::eq("source", "wiki"))
                .unwrap_err()
                .kind(),
            "unsupported"
        );
        assert_eq!(
            filter_sql(&Filter::And(Vec::new())).unwrap_err().kind(),
            "config"
        );
    }

    #[test]
    fn distance_and_index_mappings_are_stable() {
        assert_eq!(distance_type(Distance::Cosine), DistanceType::Cosine);
        assert_eq!(distance_type(Distance::L2), DistanceType::L2);
        assert_eq!(distance_type(Distance::InnerProduct), DistanceType::Dot);

        assert_eq!(index_kind(&IndexType::IvfFlat), Some(IndexKind::IvfFlat));
        assert_eq!(index_kind(&IndexType::IvfPq), Some(IndexKind::IvfPq));
        assert_eq!(index_kind(&IndexType::IvfHnswSq), Some(IndexKind::Hnsw));
        assert_eq!(index_kind(&IndexType::BTree), None);
    }
}

