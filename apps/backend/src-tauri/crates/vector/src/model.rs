//! 与驱动无关的向量数据模型
//!
//! 这里的类型是三个后端（Redis/Valkey、PostgreSQL+pgvector、LanceDB）共用的
//! 契约：只描述「表 / 向量 / 索引 / 查询」这些概念，不出现任何驱动细节。
//! 后端实现负责把这些概念翻译成各自的 SQL / RESP / SDK 调用。

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{VectorError, VectorResult};

/// 单表维度上限（pgvector 的 `vector` 类型上限 16000 维）
pub const MAX_DIMENSIONS: usize = 16_000;

/// 单次写入 / 查询的最大批量
pub const MAX_BATCH: usize = 1_000;

/// 单次查询返回的最大条数
pub const MAX_TOP_K: usize = 1_000;

/// 向量后端
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Redis / Valkey（Redis Stack 或已加载 RediSearch 的兼容实现）
    Redis,
    /// PostgreSQL + pgvector 扩展
    PgVector,
    /// LanceDB（本地目录或对象存储上的嵌入式副本）
    Lance,
}

impl Backend {
    /// 全部后端
    pub const ALL: [Self; 3] = [Self::Redis, Self::PgVector, Self::Lance];

    /// 稳定标识（与 serde 名一致，可用于配置、日志、界面 key）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Redis => "redis",
            Self::PgVector => "pgvector",
            Self::Lance => "lance",
        }
    }

    /// 中文展示名
    pub fn label(&self) -> &'static str {
        match self {
            Self::Redis => "Redis / Valkey 向量集",
            Self::PgVector => "PostgreSQL + pgvector",
            Self::Lance => "LanceDB 向量副本",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Backend {
    type Err = VectorError;

    fn from_str(value: &str) -> VectorResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "redis" | "valkey" | "redis_stack" | "redis-stack" => Ok(Self::Redis),
            "pgvector" | "pg_vector" | "postgres" | "postgresql" | "pg" => Ok(Self::PgVector),
            "lance" | "lancedb" => Ok(Self::Lance),
            other => Err(VectorError::config(format!("未知向量后端：{other}"))),
        }
    }
}

/// 距离度量（决定索引参数与相似度换算）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Distance {
    /// 余弦距离（文本嵌入最常用；pgvector 的 `vector_cosine_ops`）
    Cosine,
    /// 欧氏距离（`L2`）
    L2,
    /// 内积（`IP`，等价于负内积距离）
    InnerProduct,
}

impl Distance {
    /// 全部度量
    pub const ALL: [Self; 3] = [Self::Cosine, Self::L2, Self::InnerProduct];

    /// 稳定标识
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cosine => "cosine",
            Self::L2 => "l2",
            Self::InnerProduct => "ip",
        }
    }

    /// pgvector 的操作符类
    pub fn pgvector_ops(&self) -> &'static str {
        match self {
            Self::Cosine => "vector_cosine_ops",
            Self::L2 => "vector_l2_ops",
            Self::InnerProduct => "vector_ip_ops",
        }
    }

    /// pgvector 的距离运算符（用于 `ORDER BY col <op> $1`）
    pub fn pgvector_operator(&self) -> &'static str {
        match self {
            Self::Cosine => "<=>",
            Self::L2 => "<->",
            Self::InnerProduct => "<#>",
        }
    }

    /// RediSearch 的向量距离算法名（`FT.CREATE ... DISTANCE_METRIC`）
    pub fn redis_metric(&self) -> &'static str {
        match self {
            Self::Cosine => "COSINE",
            Self::L2 => "L2",
            Self::InnerProduct => "IP",
        }
    }

    /// 把后端返回的「距离」换算成「相似度（越大越相近，0..=1 为主）」
    pub fn similarity(&self, distance: f32) -> f32 {
        match self {
            Self::Cosine => (1.0 - distance).clamp(-1.0, 1.0),
            Self::L2 => 1.0 / (1.0 + distance.max(0.0)),
            Self::InnerProduct => 1.0 - distance,
        }
    }
}

impl std::fmt::Display for Distance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Distance {
    type Err = VectorError;

    fn from_str(value: &str) -> VectorResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cosine" | "cos" | "cosine_distance" => Ok(Self::Cosine),
            "l2" | "euclidean" | "l2sq" => Ok(Self::L2),
            "ip" | "inner_product" | "dot" => Ok(Self::InnerProduct),
            other => Err(VectorError::config(format!("未知距离度量：{other}"))),
        }
    }
}

/// 标识符校验：`[A-Za-z_][A-Za-z0-9_]*`，长度 1..=63
///
/// 所有拼进语句的表名 / 列名都先过这一关，杜绝把用户输入直接拼进 SQL / RESP。
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name.len() > 63 {
        return false;
    }
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 校验表名：允许 `schema.table` 两段式
pub fn validate_table_name(name: &str) -> VectorResult<()> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() > 2 {
        return Err(VectorError::config(format!(
            "表名最多允许 schema.table 两段：{name}"
        )));
    }
    if parts.iter().any(|part| !is_identifier(part)) {
        return Err(VectorError::config(format!(
            "表名只能是字母 / 数字 / 下划线（可带 schema 前缀）：{name}"
        )));
    }
    Ok(())
}

/// 索引类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    /// 暴力检索（不建近似索引；小规模数据或后端不支持 ANN 时的兜底）
    Flat,
    /// HNSW（图索引：查询快、内存占用高）
    Hnsw,
    /// IVF_FLAT（倒排聚类 + 精确候选）
    IvfFlat,
    /// IVF_PQ（倒排聚类 + 乘积量化：压缩率高、精度有损）
    IvfPq,
}

impl IndexKind {
    /// 全部索引类型
    pub const ALL: [Self; 4] = [Self::Flat, Self::Hnsw, Self::IvfFlat, Self::IvfPq];

    /// 稳定标识
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Hnsw => "hnsw",
            Self::IvfFlat => "ivf_flat",
            Self::IvfPq => "ivf_pq",
        }
    }

    /// pgvector 的索引访问方法；`Flat` 表示不建索引
    pub fn pgvector_access_method(&self) -> Option<&'static str> {
        match self {
            Self::Flat => None,
            Self::Hnsw => Some("hnsw"),
            Self::IvfFlat | Self::IvfPq => Some("ivfflat"),
        }
    }

    /// RediSearch 的向量索引算法；`None` 表示该后端不支持
    pub fn redis_algorithm(&self) -> Option<&'static str> {
        match self {
            Self::Flat => Some("FLAT"),
            Self::Hnsw => Some("HNSW"),
            Self::IvfFlat | Self::IvfPq => None,
        }
    }

    /// Lance 的索引类型名；`None` 表示走暴力扫描
    pub fn lance_index_type(&self) -> Option<&'static str> {
        match self {
            Self::Flat => None,
            Self::Hnsw => Some("IVF_HNSW_SQ"),
            Self::IvfFlat => Some("IVF_FLAT"),
            Self::IvfPq => Some("IVF_PQ"),
        }
    }
}

impl std::fmt::Display for IndexKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IndexKind {
    type Err = VectorError;

    fn from_str(value: &str) -> VectorResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "flat" | "brute" | "none" => Ok(Self::Flat),
            "hnsw" => Ok(Self::Hnsw),
            "ivf_flat" | "ivfflat" => Ok(Self::IvfFlat),
            "ivf_pq" | "ivfpq" => Ok(Self::IvfPq),
            other => Err(VectorError::config(format!("未知索引类型：{other}"))),
        }
    }
}

/// 索引定义（建表时使用，也可用于描述已有索引）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexSpec {
    /// 索引类型
    pub kind: IndexKind,
    /// 索引参数（如 `m`、`ef_construction`、`lists`、`num_partitions`）
    pub parameters: BTreeMap<String, String>,
}

impl Default for IndexSpec {
    fn default() -> Self {
        Self {
            kind: IndexKind::Hnsw,
            parameters: BTreeMap::new(),
        }
    }
}

impl IndexSpec {
    /// 指定索引类型
    pub fn new(kind: IndexKind) -> Self {
        Self {
            kind,
            parameters: BTreeMap::new(),
        }
    }

    /// 暴力检索（不建 ANN 索引）
    pub fn flat() -> Self {
        Self::new(IndexKind::Flat)
    }

    /// 追加一个索引参数
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    /// 校验参数名与参数值
    pub fn validate(&self) -> VectorResult<()> {
        for (key, value) in &self.parameters {
            if !is_identifier(key) {
                return Err(VectorError::config(format!("索引参数名非法：{key}")));
            }
            if value.trim().is_empty() || value.len() > 64 {
                return Err(VectorError::config(format!(
                    "索引参数 {key} 的取值必须在 1..=64 字节内"
                )));
            }
        }
        Ok(())
    }
}

/// 建表 / 写入时使用的表定义
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSpec {
    /// 表名（pgvector 允许 `schema.table` 两段式）
    pub name: String,
    /// 向量维度（1..=[`MAX_DIMENSIONS`]）
    pub dimensions: usize,
    /// 距离度量
    pub distance: Distance,
    /// 索引定义
    pub index: IndexSpec,
    /// 向量列名
    pub vector_column: String,
    /// 文本列名（空字符串表示不存原文）
    pub text_column: String,
    /// 元数据列名（JSON）
    pub metadata_column: String,
}

impl TableSpec {
    /// 默认向量列名
    pub const DEFAULT_VECTOR_COLUMN: &'static str = "embedding";
    /// 默认文本列名
    pub const DEFAULT_TEXT_COLUMN: &'static str = "content";
    /// 默认元数据列名
    pub const DEFAULT_METADATA_COLUMN: &'static str = "metadata";

    /// 新建表定义：默认 `cosine` + `HNSW` + 默认列名
    pub fn new(name: impl Into<String>, dimensions: usize) -> Self {
        Self {
            name: name.into(),
            dimensions,
            distance: Distance::Cosine,
            index: IndexSpec::default(),
            vector_column: Self::DEFAULT_VECTOR_COLUMN.to_string(),
            text_column: Self::DEFAULT_TEXT_COLUMN.to_string(),
            metadata_column: Self::DEFAULT_METADATA_COLUMN.to_string(),
        }
    }

    /// 设置距离度量
    pub fn with_distance(mut self, distance: Distance) -> Self {
        self.distance = distance;
        self
    }

    /// 设置索引定义
    pub fn with_index(mut self, index: IndexSpec) -> Self {
        self.index = index;
        self
    }

    /// 设置向量列名
    pub fn with_vector_column(mut self, column: impl Into<String>) -> Self {
        self.vector_column = column.into();
        self
    }

    /// 设置文本列名（传空字符串表示不存原文）
    pub fn with_text_column(mut self, column: impl Into<String>) -> Self {
        self.text_column = column.into();
        self
    }

    /// 设置元数据列名
    pub fn with_metadata_column(mut self, column: impl Into<String>) -> Self {
        self.metadata_column = column.into();
        self
    }

    /// 校验表名、维度、列名与索引参数
    pub fn validate(&self) -> VectorResult<()> {
        validate_table_name(&self.name)?;
        if self.dimensions == 0 || self.dimensions > MAX_DIMENSIONS {
            return Err(VectorError::config(format!(
                "维度必须在 1..={MAX_DIMENSIONS} 之间：{}",
                self.dimensions
            )));
        }
        if !is_identifier(&self.vector_column) {
            return Err(VectorError::config(format!(
                "向量列名非法：{}",
                self.vector_column
            )));
        }
        if !self.text_column.is_empty() && !is_identifier(&self.text_column) {
            return Err(VectorError::config(format!(
                "文本列名非法：{}",
                self.text_column
            )));
        }
        if !is_identifier(&self.metadata_column) {
            return Err(VectorError::config(format!(
                "元数据列名非法：{}",
                self.metadata_column
            )));
        }
        self.index.validate()
    }
}

/// 一条待写入的向量记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorRecord {
    /// 主键（同 id 再次写入即为覆盖）
    pub id: String,
    /// 向量（维度必须与表定义一致）
    pub vector: Vec<f32>,
    /// 原文（可选，便于召回后直接展示）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 元数据（BTreeMap 保证序列化顺序稳定，便于幂等与测试）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl VectorRecord {
    /// 新建记录
    pub fn new(id: impl Into<String>, vector: Vec<f32>) -> Self {
        Self {
            id: id.into(),
            vector,
            text: None,
            metadata: BTreeMap::new(),
        }
    }

    /// 附加原文
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// 追加一个元数据字段
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// 向量维度
    pub fn dimension(&self) -> usize {
        self.vector.len()
    }

    /// 校验主键与元数据字段名
    pub fn validate(&self) -> VectorResult<()> {
        if self.id.trim().is_empty() || self.id.len() > 256 {
            return Err(VectorError::config("记录主键必须在 1..=256 字节内"));
        }
        if self.vector.is_empty() {
            return Err(VectorError::config(format!("记录 {} 的向量为空", self.id)));
        }
        // 浮点 NaN / Infinity 无法写入任何后端，提前拦截
        if self.vector.iter().any(|value| !value.is_finite()) {
            return Err(VectorError::config(format!(
                "记录 {} 的向量包含 NaN / Infinity",
                self.id
            )));
        }
        for key in self.metadata.keys() {
            if !is_identifier(key) {
                return Err(VectorError::config(format!(
                    "记录 {} 的元数据字段名非法：{key}",
                    self.id
                )));
            }
        }
        Ok(())
    }
}

/// 写入结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UpsertOutcome {
    /// 影响行数（各后端语义略有差异：pgvector 为写入行数，LanceDB 为新增+更新）
    pub affected: u64,
    /// 实际提交的批次数（后端内部按 [`MAX_BATCH`] 切分）
    pub batches: usize,
}

impl UpsertOutcome {
    /// 构造写入结果
    pub fn new(affected: u64, batches: usize) -> Self {
        Self { affected, batches }
    }
}

/// 元数据过滤条件
///
/// 只做**下推**：条件交给后端在索引层过滤，不在客户端做全量回捞，
/// 否则 `top_k` 语义会被破坏（客户端过滤后条数不够）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Filter {
    /// 等值匹配
    Eq {
        /// 元数据字段名
        field: String,
        /// 期望值（统一按字符串下推）
        value: String,
    },
    /// 集合匹配（IN）
    In {
        /// 元数据字段名
        field: String,
        /// 候选值（至少一个）
        values: Vec<String>,
    },
    /// 逻辑与
    And(Vec<Filter>),
    /// 逻辑或
    Or(Vec<Filter>),
}

impl Filter {
    /// 等值条件
    pub fn eq(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Eq {
            field: field.into(),
            value: value.into(),
        }
    }

    /// IN 条件
    pub fn any_of(field: impl Into<String>, values: Vec<String>) -> Self {
        Self::In {
            field: field.into(),
            values,
        }
    }

    /// AND 组合
    pub fn and(filters: Vec<Self>) -> Self {
        Self::And(filters)
    }

    /// OR 组合
    pub fn or(filters: Vec<Self>) -> Self {
        Self::Or(filters)
    }

    /// 条件涉及的字段数（用于估算过滤强度）
    pub fn field_count(&self) -> usize {
        match self {
            Self::Eq { .. } => 1,
            Self::In { .. } => 1,
            Self::And(items) | Self::Or(items) => items.iter().map(Self::field_count).sum(),
        }
    }

    /// 递归校验字段名与候选值
    pub fn validate(&self) -> VectorResult<()> {
        match self {
            Self::Eq { field, .. } => {
                if !is_identifier(field) {
                    return Err(VectorError::config(format!("过滤字段名非法：{field}")));
                }
                Ok(())
            }
            Self::In { field, values } => {
                if !is_identifier(field) {
                    return Err(VectorError::config(format!("过滤字段名非法：{field}")));
                }
                if values.is_empty() {
                    return Err(VectorError::config(format!(
                        "过滤字段 {field} 的 IN 候选值不能为空"
                    )));
                }
                Ok(())
            }
            Self::And(items) | Self::Or(items) => {
                if items.is_empty() {
                    return Err(VectorError::config("逻辑组合不能为空"));
                }
                items.iter().try_for_each(Self::validate)
            }
        }
    }
}

/// 查询请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRequest {
    /// 查询向量
    pub vector: Vec<f32>,
    /// 期望返回条数（1..=[`MAX_TOP_K`]）
    pub top_k: usize,
    /// 元数据过滤（下推到后端）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    /// 是否回传向量本身（默认不回传，省带宽）
    #[serde(default)]
    pub include_vector: bool,
}

impl QueryRequest {
    /// 构造查询
    pub fn new(vector: Vec<f32>, top_k: usize) -> Self {
        Self {
            vector,
            top_k,
            filter: None,
            include_vector: false,
        }
    }

    /// 附加过滤条件
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// 让结果回传向量
    pub fn with_vector_in_result(mut self) -> Self {
        self.include_vector = true;
        self
    }

    /// 校验条数、向量、维度与过滤条件
    pub fn validate(&self, dimensions: usize) -> VectorResult<()> {
        if self.top_k == 0 || self.top_k > MAX_TOP_K {
            return Err(VectorError::config(format!(
                "top_k 必须在 1..={MAX_TOP_K} 之间：{}",
                self.top_k
            )));
        }
        if self.vector.is_empty() {
            return Err(VectorError::config("查询向量不能为空"));
        }
        if self.vector.len() != dimensions {
            return Err(VectorError::schema(format!(
                "查询向量维度 {} 与表维度 {dimensions} 不一致",
                self.vector.len()
            )));
        }
        if self.vector.iter().any(|value| !value.is_finite()) {
            return Err(VectorError::config("查询向量包含 NaN / Infinity"));
        }
        if let Some(filter) = &self.filter {
            filter.validate()?;
        }
        Ok(())
    }
}

/// 召回结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// 主键
    pub id: String,
    /// 相似度（越大越相近，已按距离度量换算，便于跨后端统一排序展示）
    pub score: f32,
    /// 后端原始距离（部分后端只返回换算后的 score，此时为 `None`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f32>,
    /// 向量本身（仅当请求 `include_vector` 时回传）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<Vec<f32>>,
    /// 原文
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 元数据
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl SearchHit {
    /// 只带主键与相似度的结果
    pub fn new(id: impl Into<String>, score: f32) -> Self {
        Self {
            id: id.into(),
            score,
            distance: None,
            vector: None,
            text: None,
            metadata: BTreeMap::new(),
        }
    }

    /// 用距离度量补全 `distance` 与 `score`
    pub fn from_distance(id: impl Into<String>, distance: f32, metric: Distance) -> Self {
        let mut hit = Self::new(id, metric.similarity(distance));
        hit.distance = Some(distance);
        hit
    }
}

/// 把向量渲染成 pgvector 文本字面量：`[0.1,0.2,0.3]`
///
/// 用 Rust 的 `{}` 浮点格式化（最短往返表示），避免精度损失。
pub fn vector_literal(vector: &[f32]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(vector.len() * 12 + 2);
    out.push('[');
    for (index, value) in vector.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // 忽略写入错误：String 的 write! 不会失败
        let _ = write!(out, "{value}");
    }
    out.push(']');
    out
}

/// 解析后端返回的向量文本：容忍 `[1,2]`、`1,2`、空格与换行
pub fn parse_vector_literal(text: &str) -> VectorResult<Vec<f32>> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|part| {
            let part = part.trim();
            part.parse::<f32>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| VectorError::backend(format!("向量元素非法：{part}")))
        })
        .collect()
}

/// 校验一批记录：主键、向量有限性、维度一致、批量上限
pub fn check_records(spec: &TableSpec, records: &[VectorRecord]) -> VectorResult<()> {
    if records.is_empty() {
        return Err(VectorError::config("写入记录不能为空"));
    }
    if records.len() > MAX_BATCH {
        return Err(VectorError::config(format!(
            "单批最多 {MAX_BATCH} 条，实际 {} 条",
            records.len()
        )));
    }
    for record in records {
        record.validate()?;
        if record.dimension() != spec.dimensions {
            return Err(VectorError::schema(format!(
                "记录 {} 的维度 {} 与表 {} 的维度 {} 不一致",
                record.id,
                record.dimension(),
                spec.name,
                spec.dimensions
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_rules() {
        assert!(is_identifier("embedding"));
        assert!(is_identifier("_data_lance"));
        assert!(is_identifier("a1_2"));
        assert!(is_identifier(&"a".repeat(63)));
        assert!(!is_identifier(""));
        assert!(!is_identifier("1abc"));
        assert!(!is_identifier("with space"));
        assert!(!is_identifier("has-dash"));
        assert!(!is_identifier("quote'"));
        assert!(!is_identifier(&"a".repeat(64)));
    }

    #[test]
    fn table_name_allows_schema_prefix_only() {
        assert!(validate_table_name("chunks").is_ok());
        assert!(validate_table_name("public.chunks").is_ok());
        assert!(validate_table_name("a.b.c").is_err());
        assert!(validate_table_name("public.").is_err());
        assert!(validate_table_name("chunks; drop table x").is_err());
    }

    #[test]
    fn table_spec_validation() {
        let spec = TableSpec::new("chunks", 768);
        assert!(spec.validate().is_ok());
        assert_eq!(spec.distance, Distance::Cosine);
        assert_eq!(spec.index.kind, IndexKind::Hnsw);
        assert_eq!(spec.vector_column, "embedding");
        assert_eq!(spec.text_column, "content");
        assert_eq!(spec.metadata_column, "metadata");

        assert!(TableSpec::new("chunks", 0).validate().is_err());
        assert!(TableSpec::new("chunks", MAX_DIMENSIONS + 1).validate().is_err());
        assert!(TableSpec::new("bad name", 8).validate().is_err());

        // 空文本列 = 不存原文
        assert!(TableSpec::new("chunks", 8).with_text_column("").validate().is_ok());
        // 列名非法
        assert!(
            TableSpec::new("chunks", 8)
                .with_vector_column("vec col")
                .validate()
                .is_err()
        );
        // 索引参数值不能为空
        assert!(
            TableSpec::new("chunks", 8)
                .with_index(IndexSpec::new(IndexKind::IvfFlat).with_param("lists", ""))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn distance_similarity_and_backend_mapping() {
        assert!((Distance::Cosine.similarity(0.0) - 1.0).abs() < 1e-6);
        assert!((Distance::Cosine.similarity(2.0) + 1.0).abs() < 1e-6);
        assert!((Distance::L2.similarity(0.0) - 1.0).abs() < 1e-6);
        assert!(Distance::L2.similarity(3.0) < 0.3);
        assert_eq!(Distance::Cosine.pgvector_ops(), "vector_cosine_ops");
        assert_eq!(Distance::L2.pgvector_operator(), "<->");
        assert_eq!(Distance::InnerProduct.redis_metric(), "IP");
        assert_eq!("cosine".parse::<Distance>().unwrap(), Distance::Cosine);
        assert_eq!("euclidean".parse::<Distance>().unwrap(), Distance::L2);
        assert!("nope".parse::<Distance>().is_err());
    }

    #[test]
    fn index_kind_round_trips_and_backend_mapping() {
        for kind in IndexKind::ALL {
            assert_eq!(kind.as_str().parse::<IndexKind>().unwrap(), kind);
        }
        assert_eq!(IndexKind::Hnsw.pgvector_access_method(), Some("hnsw"));
        assert_eq!(IndexKind::Flat.pgvector_access_method(), None);
        assert_eq!(IndexKind::Hnsw.redis_algorithm(), Some("HNSW"));
        assert_eq!(IndexKind::IvfPq.redis_algorithm(), None);
        assert_eq!(IndexKind::IvfPq.lance_index_type(), Some("IVF_PQ"));
        assert_eq!("ivfflat".parse::<IndexKind>().unwrap(), IndexKind::IvfFlat);
    }

    #[test]
    fn backend_labels_and_parsing() {
        for backend in Backend::ALL {
            assert_eq!(backend.as_str().parse::<Backend>().unwrap(), backend);
            assert!(!backend.label().is_empty());
            assert_eq!(backend.to_string(), backend.as_str());
        }
        assert_eq!("valkey".parse::<Backend>().unwrap(), Backend::Redis);
        assert_eq!("postgres".parse::<Backend>().unwrap(), Backend::PgVector);
        assert_eq!("lancedb".parse::<Backend>().unwrap(), Backend::Lance);
        assert!("mile".parse::<Backend>().is_err());
    }

    #[test]
    fn record_validation_rejects_bad_input() {
        let record = VectorRecord::new("doc-1", vec![0.1, 0.2])
            .with_text("hello")
            .with_metadata("source", "wiki");
        assert!(record.validate().is_ok());
        assert_eq!(record.dimension(), 2);
        assert_eq!(record.text.as_deref(), Some("hello"));
        assert_eq!(
            record.metadata.get("source"),
            Some(&serde_json::json!("wiki"))
        );

        assert!(VectorRecord::new("", vec![0.1]).validate().is_err());
        assert!(VectorRecord::new("doc", Vec::new()).validate().is_err());
        assert!(VectorRecord::new("doc", vec![f32::NAN]).validate().is_err());
        assert!(VectorRecord::new("doc", vec![f32::INFINITY]).validate().is_err());
        assert!(
            VectorRecord::new("doc", vec![0.1])
                .with_metadata("bad key", "v")
                .validate()
                .is_err()
        );
        assert!(VectorRecord::new("x".repeat(257), vec![0.1]).validate().is_err());
    }

    #[test]
    fn filter_validation_and_field_count() {
        let filter = Filter::and(vec![
            Filter::eq("lang", "zh"),
            Filter::any_of("team", vec!["a".to_string(), "b".to_string()]),
        ]);
        assert!(filter.validate().is_ok());
        assert_eq!(filter.field_count(), 2);

        assert!(Filter::eq("bad field", "v").validate().is_err());
        assert!(Filter::any_of("team", Vec::new()).validate().is_err());
        assert!(Filter::and(Vec::new()).validate().is_err());
        assert!(
            Filter::or(vec![Filter::eq("ok", "1"), Filter::eq("no way", "2")])
                .validate()
                .is_err()
        );
    }

    #[test]
    fn query_request_validation() {
        let request = QueryRequest::new(vec![1.0, 0.0, 0.0], 5);
        assert!(request.validate(3).is_ok());
        assert!(!request.include_vector);

        assert!(QueryRequest::new(vec![1.0], 0).validate(1).is_err());
        assert!(
            QueryRequest::new(vec![1.0], MAX_TOP_K + 1)
                .validate(1)
                .is_err()
        );
        assert!(QueryRequest::new(Vec::new(), 1).validate(0).is_err());
        assert!(QueryRequest::new(vec![1.0, 2.0], 1).validate(3).is_err());
        assert!(QueryRequest::new(vec![f32::NAN], 1).validate(1).is_err());
        assert!(
            QueryRequest::new(vec![1.0], 1)
                .with_filter(Filter::eq("bad field", "v"))
                .validate(1)
                .is_err()
        );
        assert!(
            QueryRequest::new(vec![1.0], 1)
                .with_vector_in_result()
                .include_vector
        );
    }

    #[test]
    fn vector_literal_round_trip() {
        let literal = vector_literal(&[0.5, -1.25, 3.0]);
        assert_eq!(literal, "[0.5,-1.25,3]");
        assert_eq!(
            parse_vector_literal(&literal).unwrap(),
            vec![0.5, -1.25, 3.0]
        );
        assert_eq!(parse_vector_literal(" 1 , 2 ").unwrap(), vec![1.0, 2.0]);
        assert_eq!(parse_vector_literal("[]").unwrap(), Vec::<f32>::new());
        assert!(parse_vector_literal("[1, oops]").is_err());
        assert!(parse_vector_literal("[1, NaN]").is_err());
        assert!(parse_vector_literal("[1, inf]").is_err());
    }

    #[test]
    fn check_records_enforces_batch_and_dimension() {
        let spec = TableSpec::new("chunks", 3);
        let ok = vec![VectorRecord::new("a", vec![1.0, 0.0, 0.0])];
        assert!(check_records(&spec, &ok).is_ok());

        let wrong_dimension = vec![VectorRecord::new("a", vec![1.0, 0.0])];
        assert_eq!(
            check_records(&spec, &wrong_dimension).unwrap_err().kind(),
            "schema"
        );

        assert_eq!(check_records(&spec, &[]).unwrap_err().kind(), "config");

        let too_many: Vec<VectorRecord> = (0..=MAX_BATCH)
            .map(|index| VectorRecord::new(format!("id-{index}"), vec![0.0; 3]))
            .collect();
        assert_eq!(
            check_records(&spec, &too_many).unwrap_err().kind(),
            "config"
        );
    }

    #[test]
    fn search_hit_converts_distance_to_score() {
        let hit = SearchHit::from_distance("doc-1", 0.25, Distance::Cosine);
        assert_eq!(hit.distance, Some(0.25));
        assert!((hit.score - 0.75).abs() < 1e-6);
        assert_eq!(SearchHit::new("doc-2", 0.5).distance, None);
        let l2 = SearchHit::from_distance("doc-3", 1.0, Distance::L2);
        assert!((l2.score - 0.5).abs() < 1e-6);
    }
}
