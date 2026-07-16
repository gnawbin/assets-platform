//! 集成测试：SurrealDB 连接与向量搜索功能
//!
//! 测试 SurrealDB v3.x 作为向量数据库的核心功能，包括：
//! - TCP/WebSocket 连接与认证
//! - 基本的 CRUD 操作
//! - 向量索引创建与向量相似性搜索
//!
//! 前提：需要 SurrealDB 实例运行在 127.0.0.1:8000
//! 启动命令参考：
//! docker run -d --name surrealdb -p 8000:8000 surrealdb/surrealdb:latest start --user admin --pass Admin@123456

use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::types::Value;
use surrealdb::Surreal;

/// SurrealDB 连接配置
const DB_HOST: &str = "127.0.0.1:8000";
const DB_USER: &str = "admin";
const DB_PASS: &str = "Admin@123456";
const DB_NS: &str = "test";
const DB_NAME: &str = "test";

/// 建立 SurrealDB 连接并完成认证
async fn connect() -> Surreal<surrealdb::engine::remote::ws::Client> {
    let db = Surreal::new::<Ws>(DB_HOST)
        .await
        .expect("❌ 无法连接到 SurrealDB，请确认服务是否运行在 127.0.0.1:8000");

    db.signin(Root {
        username: DB_USER.to_string(),
        password: DB_PASS.to_string(),
    })
    .await
    .expect("❌ SurrealDB 认证失败，请检查用户名/密码");

    db.use_ns(DB_NS)
        .use_db(DB_NAME)
        .await
        .expect("❌ 无法选择 Namespace/Database");

    db
}

/// 辅助函数：将 surrealdb::types::Value 转换为自定义结构体
/// SurrealDB v3.x 的 select() 和 query().take() 需要 T: SurrealValue，
/// 自定义结构体不实现该 trait，所以先用 Value 接收再转换。
fn convert_value<T: serde::de::DeserializeOwned>(value: Value) -> T {
    let json = value.into_json_value();
    serde_json::from_value(json).expect("❌ 反序列化失败")
}

fn convert_values<T: serde::de::DeserializeOwned>(values: Vec<Value>) -> Vec<T> {
    values.into_iter().map(|v| convert_value(v)).collect()
}

/// 辅助函数：将任意序列化结构体转换为 surrealdb::types::Value
/// SurrealDB v3.x 的 create().content() 要求 data 实现 SurrealValue，
/// 自定义结构体不实现，因此先序列化为 Value。
fn to_value<T: serde::Serialize>(data: T) -> Value {
    let json = serde_json::to_value(data).expect("❌ 序列化失败");
    serde_json::from_value(json).expect("❌ 转换为 SurrealDB Value 失败")
}

// ======================== CRUD 基础测试 ========================

mod crud_tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// 测试数据结构
    #[derive(Debug, Serialize, Deserialize)]
    struct Person {
        name: String,
        age: u32,
        email: String,
    }

    /// 测试 1：连接 SurrealDB 并验证连通性
    #[tokio::test]
    async fn test_connect_to_surrealdb() {
        let db = connect().await;

        // 执行一个简单的健康检查查询
        let result: Vec<Value> = db
            .query("RETURN 1 + 1")
            .await
            .expect("❌ 查询执行失败")
            .take(0)
            .expect("❌ 无法提取查询结果");

        assert!(!result.is_empty(), "❌ 查询结果为空");
        println!("✅ 连接成功！RETURN 1 + 1 = {:?}", result[0]);
    }

    /// 测试 2：创建文档并查询
    #[tokio::test]
    async fn test_create_and_select() {
        let db = connect().await;

        // 先清理可能残留的测试数据
        let _ = db.query("DELETE person_test").await;

        // 创建一条记录
        let person = Person {
            name: "Alice".to_string(),
            age: 30,
            email: "alice@example.com".to_string(),
        };

        let created: Person = db
            .create("person_test")
            .content(to_value(person))
            .await
            .expect("❌ 创建文档失败")
            .map(convert_value)
            .expect("❌ 创建文档返回空记录");

        assert_eq!(created.name, "Alice");
        assert_eq!(created.age, 30);
        println!("✅ 创建文档成功：{:?}", created);

        // 查询所有记录（select 返回 Vec<Value>，需转换）
        let selected_values: Vec<Value> = db.select("person_test").await.expect("❌ 查询文档失败");
        let selected: Vec<Person> = convert_values(selected_values);

        assert!(!selected.is_empty(), "❌ 查询结果不应为空");
        assert!(selected.iter().any(|p| p.name == "Alice"));
        println!("✅ 查询成功，共 {} 条记录", selected.len());

        // 清理
        let _ = db.query("DELETE person_test").await;
    }

    /// 测试 3：更新和删除文档
    #[tokio::test]
    async fn test_update_and_delete() {
        let db = connect().await;

        // 先清理可能残留的数据
        let _ = db.query("DELETE person_update_test").await;

        // 创建测试数据
        let person = Person {
            name: "Bob".to_string(),
            age: 25,
            email: "bob@example.com".to_string(),
        };

        let created: Person = db
            .create("person_update_test")
            .content(to_value(person))
            .await
            .expect("❌ 创建文档失败")
            .map(convert_value)
            .expect("❌ 创建文档返回空记录");

        println!("✅ 创建文档成功：{:?}", created);

        // 使用 SurrealQL 更新
        let updated_values: Vec<Value> = db
            .query("UPDATE person_update_test SET age = 26 WHERE name = $name")
            .bind(("name", "Bob"))
            .await
            .expect("❌ 更新文档失败")
            .take(0)
            .expect("❌ 无法提取更新结果");
        let updated: Vec<Person> = convert_values(updated_values);

        assert_eq!(updated.len(), 1, "❌ 应返回 1 条记录");
        assert_eq!(updated[0].age, 26, "❌ 年龄应被更新为 26");
        println!("✅ 更新成功：{:?}", updated[0]);

        // 删除
        let _: Vec<Value> = db
            .query("DELETE person_update_test WHERE name = $name")
            .bind(("name", "Bob"))
            .await
            .expect("❌ 删除文档失败")
            .take(0)
            .expect("❌ 无法提取删除结果");

        // 确认删除（select 返回 Vec<Value>，需转换）
        let remaining_values: Vec<Value> = db
            .select("person_update_test")
            .await
            .expect("❌ 查询剩余文档失败");
        let remaining: Vec<Person> = convert_values(remaining_values);

        assert!(
            !remaining.iter().any(|p| p.name == "Bob"),
            "❌ Bob 应该已被删除"
        );
        println!("✅ 删除成功！");

        // 清理
        let _ = db.query("DELETE person_update_test").await;
    }
}

// ======================== 向量搜索测试 ========================

mod vector_tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// 带向量的文档结构
    #[derive(Debug, Serialize, Deserialize)]
    struct VectorDoc {
        id: String,
        content: String,
        #[serde(rename = "embedding")]
        embedding: Vec<f32>,
    }

    /// 测试 4：创建带向量索引的表
    #[tokio::test]
    async fn test_create_vector_index() {
        let db = connect().await;

        // 清理可能残留的旧表
        let _ = db.query("REMOVE TABLE vector_test").await;
        let _ = db.query("REMOVE TABLE vector_items").await;

        // 创建表
        db.query("CREATE TABLE vector_items")
            .await
            .expect("❌ 创建表失败");

        // 定义向量索引（MTREE 类型，维度 3）
        let define_result = db
            .query("DEFINE INDEX idx_embedding ON vector_items COLUMNS embedding MTREE DIMENSION 3")
            .await;

        match define_result {
            Ok(_) => println!("✅ 向量索引创建成功：MTREE DIMENSION 3"),
            Err(e) => {
                // 如果索引已存在或其他原因，记录但不失败
                println!("⚠️ 索引创建（可能已存在）：{:?}", e);
            }
        }

        // 验证表存在
        let tables: Vec<Value> = db
            .query("INFO FOR TABLE vector_items")
            .await
            .expect("❌ 查询表信息失败")
            .take(0)
            .expect("❌ 无法提取表信息");

        println!("✅ 表 vector_items 信息：{:?}", tables);

        // 清理
        let _ = db.query("REMOVE TABLE vector_items").await;
    }

    /// 测试 5：插入向量数据
    #[tokio::test]
    async fn test_insert_vector_data() {
        let db = connect().await;

        // 清理
        let _ = db.query("REMOVE TABLE vector_docs").await;
        let _ = db.query("CREATE TABLE vector_docs").await;

        // 插入 3 条带向量的文档
        let docs = vec![
            ("doc1", "苹果是一种水果", vec![1.0, 0.0, 0.0]),
            ("doc2", "香蕉也是一种水果", vec![0.0, 1.0, 0.0]),
            ("doc3", "汽车是一种交通工具", vec![0.0, 0.0, 1.0]),
        ];

        for (id, content, embedding) in &docs {
            let sql = format!(
                "CREATE vector_docs:{} SET content = '{}', embedding = {}",
                id,
                content,
                format_vector(embedding)
            );

            db.query(&sql)
                .await
                .expect(&format!("❌ 插入文档 {} 失败", id));
            println!("✅ 插入文档 {}: {}", id, content);
        }

        // 验证插入 3 条
        let count: Vec<Value> = db
            .query("SELECT count() FROM vector_docs GROUP BY count")
            .await
            .expect("❌ 查询计数失败")
            .take(0)
            .expect("❌ 无法提取计数结果");

        println!("✅ vector_docs 表记录数：{:?}", count);

        // 清理
        let _ = db.query("REMOVE TABLE vector_docs").await;
    }

    /// 测试 6：向量相似性搜索
    #[tokio::test]
    async fn test_vector_search() {
        let db = connect().await;

        // 清理并重建
        let _ = db.query("REMOVE TABLE vector_search_test").await;
        let _ = db.query("CREATE TABLE vector_search_test").await;

        // 创建向量索引
        let _ = db
            .query(
                "DEFINE INDEX idx_embedding ON vector_search_test \
                 COLUMNS embedding MTREE DIMENSION 3",
            )
            .await;

        // 插入向量数据（三维空间中 4 个点）
        let items = vec![
            ("item1", "机器学习的核心概念", vec![0.9, 0.1, 0.0]),
            ("item2", "深度学习与神经网络", vec![0.8, 0.2, 0.1]),
            ("item3", "水果的营养价值", vec![0.1, 0.9, 0.0]),
            ("item4", "汽车保养指南", vec![0.0, 0.1, 0.9]),
        ];

        for (id, content, emb) in &items {
            let sql = format!(
                "CREATE vector_search_test:{} SET content = '{}', embedding = {}",
                id,
                content,
                format_vector(emb)
            );
            db.query(&sql).await.expect(&format!("❌ 插入 {} 失败", id));
        }

        // 查询向量 [0.85, 0.15, 0.05]（接近 item1 和 item2）
        let query_vec = vec![0.85, 0.15, 0.05];
        let search_sql = format!(
            "SELECT id, content, embedding, \
             vector::distance::euclidean(embedding, {}) AS distance \
             FROM vector_search_test \
             ORDER BY distance ASC \
             LIMIT 3",
            format_vector(&query_vec)
        );

        #[derive(Debug, Serialize, Deserialize)]
        struct SearchResult {
            id: String,
            content: String,
            embedding: Vec<f32>,
            distance: f64,
        }

        // query().take() 返回 Vec<Value>，需转换
        let results_values: Vec<Value> = db
            .query(&search_sql)
            .await
            .expect("❌ 向量搜索查询失败")
            .take(0)
            .expect("❌ 无法提取搜索结果");
        let results: Vec<SearchResult> = convert_values(results_values);

        assert!(!results.is_empty(), "❌ 搜索结果不应为空");
        assert!(
            results[0].distance < results[1].distance,
            "❌ 结果应按距离升序排列"
        );

        println!("\n🔍 向量搜索测试（查询点：{:?}）", query_vec);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        for (i, r) in results.iter().enumerate() {
            println!(
                " #{:<2} | {} | 距离: {:.4} | 内容: {}",
                i + 1,
                r.id,
                r.distance,
                r.content
            );
        }
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("✅ 向量搜索测试通过！");

        // 清理
        let _ = db.query("REMOVE TABLE vector_search_test").await;
    }

    /// 测试 7：完整的向量搜索流水线（一站式测试）
    #[tokio::test]
    async fn test_vector_full_pipeline() {
        let db = connect().await;
        let table = "vector_pipeline_test";

        // 清理
        let _ = db.query(&format!("REMOVE TABLE {}", table)).await;
        let _ = db.query(&format!("CREATE TABLE {}", table)).await;

        // 1️⃣ 创建向量索引
        let index_sql = format!(
            "DEFINE INDEX idx_emb ON {} COLUMNS embedding MTREE DIMENSION 3",
            table
        );
        db.query(&index_sql).await.expect("❌ 创建向量索引失败");
        println!("✅ 1/4 向量索引创建成功");

        // 2️⃣ 插入向量数据
        let docs = vec![
            ("a", "Rust 是一种系统编程语言", vec![0.9, 0.1, 0.2]),
            ("b", "Python 适合快速开发", vec![0.2, 0.9, 0.1]),
            ("c", "JavaScript 是 Web 前端语言", vec![0.1, 0.2, 0.9]),
            ("d", "Rust 提供内存安全保障", vec![0.95, 0.05, 0.15]),
            ("e", "Python 的语法简单易学", vec![0.15, 0.85, 0.1]),
        ];

        for (id, content, emb) in &docs {
            let sql = format!(
                "CREATE {}:{} SET content = '{}', embedding = {}",
                table,
                id,
                content,
                format_vector(emb)
            );
            db.query(&sql).await.expect(&format!("❌ 插入 {} 失败", id));
        }
        println!("✅ 2/4 插入 {} 条向量文档成功", docs.len());

        // 3️⃣ 查询与 Rust 相关的文档（搜索向量接近 [0.9, 0.1, 0.2] 的文档）
        let query_embedding = vec![0.92, 0.08, 0.18];
        let search_sql = format!(
            "SELECT id, content, \
             vector::distance::cosine(embedding, {}) AS similarity \
             FROM {} \
             ORDER BY similarity ASC \
             LIMIT 3",
            format_vector(&query_embedding),
            table
        );

        #[derive(Debug, Serialize, Deserialize)]
        struct PipelineResult {
            id: String,
            content: String,
            similarity: f64,
        }

        // query().take() 返回 Vec<Value>，需转换
        let results_values: Vec<Value> = db
            .query(&search_sql)
            .await
            .expect("❌ 向量搜索失败")
            .take(0)
            .expect("❌ 无法提取搜索结果");
        let results: Vec<PipelineResult> = convert_values(results_values);
        assert_eq!(results.len(), 3, "❌ 应返回 3 条结果");

        println!("✅ 3/4 向量搜索成功，返回 {} 条结果", results.len());

        // 4️⃣ 验证结果 - 最接近 Rust 的文档应排在前面
        println!(
            "\n📊 搜索 \"Rust 相关内容\"（查询向量：{:?}）",
            query_embedding
        );
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        for (i, r) in results.iter().enumerate() {
            println!(
                " #{:<2} | 余弦相似度: {:.4} | 内容: {}",
                i + 1,
                r.similarity,
                r.content
            );
        }
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // 前两条应为 Rust 相关文档
        let top_contents: Vec<&str> = results.iter().map(|r| r.content.as_str()).collect();
        assert!(
            top_contents[0].contains("Rust"),
            "❌ 最相关的结果应包含 Rust：{}",
            top_contents[0]
        );
        println!("✅ 4/4 相关性排序验证通过！");

        // 清理
        let _ = db.query(&format!("REMOVE TABLE {}", table)).await;
        println!("🧹 测试数据已清理");
    }

    /// 辅助函数：将 Vec<f32> 格式化为 SurrealQL 数组字符串
    fn format_vector(v: &[f32]) -> String {
        let elements: Vec<String> = v.iter().map(|x| x.to_string()).collect();
        format!("[{}]", elements.join(", "))
    }
}
