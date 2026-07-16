//! 集成测试：langchain-rust + langgraph
//!
//! 测试 langchain-rust v4.6.0 和 langgraph v0.2.5 的核心功能。
//! 这些测试不依赖真实的 LLM API，使用同步/异步闭包模拟 LLM 行为。

// ======================== langchain-rust 测试 ========================

mod langchain_tests {
    use langchain_rust::{
        prompt::{PromptFromatter, PromptTemplate, TemplateFormat},
        prompt_args, template_fstring, template_jinja2,
    };

    /// 测试 PromptTemplate 的 FString 格式化
    #[test]
    fn test_prompt_template_fstring() {
        let template = PromptTemplate::new(
            "Hello {name}!".to_string(),
            vec!["name".to_string()],
            TemplateFormat::FString,
        );

        let input = prompt_args! {
            "name" => "world",
        };
        let result = template.format(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello world!");
    }

    /// 测试 PromptTemplate 的 Jinja2 格式化
    #[test]
    fn test_prompt_template_jinja2() {
        let template = PromptTemplate::new(
            "Hello {{name}}!".to_string(),
            vec!["name".to_string()],
            TemplateFormat::Jinja2,
        );

        let input = prompt_args! {
            "name" => "Rust",
        };
        let result = template.format(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello Rust!");
    }

    /// 测试缺失变量时 PromptTemplate 返回错误
    #[test]
    fn test_prompt_template_missing_variable() {
        let template = PromptTemplate::new(
            "Hello {name}! You are {age} years old.".to_string(),
            vec!["name".to_string(), "age".to_string()],
            TemplateFormat::FString,
        );

        // 只传入 name，缺少 age
        let input = prompt_args! {
            "name" => "Alice",
        };
        let result = template.format(input);
        assert!(result.is_err());
    }

    /// 测试 template_fstring 宏
    #[test]
    fn test_template_fstring_macro() {
        let tpl = template_fstring!("{product} costs {price} dollars.", "product", "price");
        let input = prompt_args! {
            "product" => "book",
            "price" => "29",
        };
        let result = tpl.format(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "book costs 29 dollars.");
    }

    /// 测试 template_jinja2 宏
    #[test]
    fn test_template_jinja2_macro() {
        let tpl = template_jinja2!("{{greeting}}, {{who}}!", "greeting", "who");
        let input = prompt_args! {
            "greeting" => "Hi",
            "who" => "LangChain",
        };
        let result = tpl.format(input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hi, LangChain!");
    }

    /// 测试 prompt_args! 宏
    #[test]
    fn test_prompt_args_macro() {
        let empty = prompt_args! {};
        assert!(empty.is_empty());

        let args = prompt_args! {
            "key1" => "value1",
            "key2" => 42,
        };
        assert_eq!(args.len(), 2);
        assert_eq!(args.get("key1").unwrap(), &serde_json::json!("value1"));
        assert_eq!(args.get("key2").unwrap(), &serde_json::json!(42));
    }

    /// 测试 PromptTemplate 的 FormatPrompter trait（生成 HumanMessage）
    #[test]
    fn test_prompt_template_format_prompt() {
        use langchain_rust::prompt::FormatPrompter;

        let template = PromptTemplate::new(
            "Tell me about {topic}".to_string(),
            vec!["topic".to_string()],
            TemplateFormat::FString,
        );

        let input = prompt_args! {
            "topic" => "Rust programming",
        };
        let prompt_value = template.format_prompt(input).unwrap();
        let messages = prompt_value.to_chat_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].message_type,
            langchain_rust::schemas::MessageType::HumanMessage
        );
        assert_eq!(messages[0].content, "Tell me about Rust programming");
    }
}

// ======================== langgraph 测试 ========================

mod langgraph_tests {
    use langgraph::{
        channels::{base::Channel, EphemeralValue, LastValue},
        prelude::*,
        runnable::{pipe, RunnableCallable, RunnableSeq},
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// 测试创建 StateGraph 并添加节点和边
    #[test]
    fn test_graph_creation() {
        // 创建 channels（至少需要一个状态 channel）
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "data".to_string(),
            Box::new(LastValue::new("data")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        // 添加节点
        graph
            .add_node("node_a", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph
            .add_node("node_b", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move {
                    let n = input.as_i64().unwrap_or(0);
                    Ok(JsonValue::Number(serde_json::Number::from(n * 2)))
                })
            })
            .unwrap();

        // 添加边
        graph.add_edge(START, "node_a").unwrap();
        graph.add_edge("node_a", "node_b").unwrap();
        graph.add_edge("node_b", END).unwrap();

        // 编译图
        let compiled = graph.compile().unwrap();

        // 验证图结构
        assert!(compiled.has_node("node_a"));
        assert!(compiled.has_node("node_b"));
        let node_names = compiled.node_names();
        assert!(node_names.contains(&"node_a".to_string()));
        assert!(node_names.contains(&"node_b".to_string()));
        assert_eq!(compiled.name(), "StateGraph");
    }

    /// 测试使用 SyncNodeFn 添加同步节点
    #[test]
    fn test_graph_with_sync_node() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "result".to_string(),
            Box::new(LastValue::new("result")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        // 使用同步节点函数（通过 node_fn! 宏或闭包）
        graph
            .add_node(
                "double",
                node_fn!(|input: &JsonValue, _config: &RunnableConfig| {
                    let n = input.as_i64().unwrap_or(1);
                    Ok(JsonValue::Number(serde_json::Number::from(n * 2)))
                }),
            )
            .unwrap();

        graph.add_edge(START, "double").unwrap();
        graph.add_edge("double", END).unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.has_node("double"));
    }

    /// 测试使用 coerce_to_runnable 添加 Runnable
    #[test]
    fn test_graph_with_coerce_to_runnable() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "value".to_string(),
            Box::new(LastValue::new("value")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        let add_one = coerce_to_runnable("add_one", |input: JsonValue, _config: RunnableConfig| {
            Box::pin(async move {
                let n = input.as_i64().unwrap_or(0);
                Ok(JsonValue::Number(serde_json::Number::from(n + 1)))
            })
        });

        graph.add_node("add_one", add_one).unwrap();

        graph.add_edge(START, "add_one").unwrap();
        graph.add_edge("add_one", END).unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.has_node("add_one"));
    }

    /// 测试 RunnableCallable（同步和异步）
    #[test]
    fn test_runnable_callable() {
        // 同步 Runnable
        let sync_runnable = RunnableCallable::new_sync("sync_double", |input, _config| {
            let n = input.as_i64().unwrap_or(0);
            Ok(JsonValue::Number(serde_json::Number::from(n * 2)))
        });

        let result = sync_runnable
            .invoke(
                &JsonValue::Number(serde_json::Number::from(5)),
                &RunnableConfig::new(),
            )
            .unwrap();
        assert_eq!(result, JsonValue::Number(serde_json::Number::from(10)));
        assert_eq!(sync_runnable.name(), "sync_double");
    }

    /// 测试 RunnableSeq（管道组合）
    #[test]
    fn test_runnable_seq() {
        let add_one = Arc::new(RunnableCallable::new_sync("add_one", |input, _config| {
            let n = input.as_i64().unwrap_or(0);
            Ok(JsonValue::Number(serde_json::Number::from(n + 1)))
        })) as Arc<dyn Runnable>;

        let double = Arc::new(RunnableCallable::new_sync("double", |input, _config| {
            let n = input.as_i64().unwrap_or(0);
            Ok(JsonValue::Number(serde_json::Number::from(n * 2)))
        })) as Arc<dyn Runnable>;

        let seq = RunnableSeq::new("add_then_double", vec![add_one, double]);

        // (3 + 1) * 2 = 8
        let result = seq
            .invoke(
                &JsonValue::Number(serde_json::Number::from(3)),
                &RunnableConfig::new(),
            )
            .unwrap();
        assert_eq!(result, JsonValue::Number(serde_json::Number::from(8)));
    }

    /// 测试 pipe 函数
    #[test]
    fn test_pipe_function() {
        let a = Arc::new(RunnableCallable::new_sync("a", |input, _| {
            let n = input.as_i64().unwrap_or(0);
            Ok(JsonValue::Number(serde_json::Number::from(n + 1)))
        })) as Arc<dyn Runnable>;

        let b = Arc::new(RunnableCallable::new_sync("b", |input, _| {
            let n = input.as_i64().unwrap_or(0);
            Ok(JsonValue::Number(serde_json::Number::from(n * 3)))
        })) as Arc<dyn Runnable>;

        let seq = pipe(a, b);
        assert_eq!(seq.name(), "a|b");
        assert_eq!(seq.len(), 2);

        // (2 + 1) * 3 = 9
        let result = seq
            .invoke(
                &JsonValue::Number(serde_json::Number::from(2)),
                &RunnableConfig::new(),
            )
            .unwrap();
        assert_eq!(result, JsonValue::Number(serde_json::Number::from(9)));
    }

    /// 测试 coerce_to_runnable
    #[test]
    fn test_coerce_to_runnable_helpers() {
        let async_r = coerce_to_runnable("echo", |input: JsonValue, _config: RunnableConfig| {
            Box::pin(async move { Ok(input) })
        });
        let result = async_r
            .invoke(
                &JsonValue::String("hello".to_string()),
                &RunnableConfig::new(),
            )
            .unwrap();
        assert_eq!(result, JsonValue::String("hello".to_string()));
    }

    /// 测试 LastValue channel
    #[test]
    fn test_last_value_channel() {
        let channel = LastValue::new("test");
        assert_eq!(channel.name(), "test");

        // 初始 checkpoint 应为 None
        assert!(channel.checkpoint().is_none());

        // 更新值
        channel.update(&[json!("hello")]).unwrap();
        assert_eq!(channel.checkpoint(), Some(json!("hello")));

        // 再次更新
        channel.update(&[json!("world")]).unwrap();
        assert_eq!(channel.checkpoint(), Some(json!("world")));
    }

    /// 测试 EphemeralValue channel
    #[test]
    fn test_ephemeral_value_channel() {
        let channel = EphemeralValue::new("tmp", true);
        assert_eq!(channel.name(), "tmp");
        assert!(channel.checkpoint().is_none());

        channel.update(&[json!(42)]).unwrap();
        assert_eq!(channel.checkpoint(), Some(json!(42)));
    }

    /// 测试 add_conditional_edges（条件分支）
    #[test]
    fn test_conditional_edges() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "branch".to_string(),
            Box::new(LastValue::new("branch")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        graph
            .add_node("router", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph
            .add_node("path_a", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph
            .add_node("path_b", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph.add_edge(START, "router").unwrap();

        // 条件路由：根据状态决定走 path_a 还是 path_b
        let mut path_map = HashMap::new();
        path_map.insert("a".to_string(), "path_a".to_string());
        path_map.insert("b".to_string(), "path_b".to_string());

        graph
            .add_conditional_edges(
                "router",
                |input: JsonValue, _config: RunnableConfig| {
                    Box::pin(async move {
                        let val = input.as_str().unwrap_or("b").to_string();
                        Ok(JsonValue::String(val))
                    })
                },
                Some(path_map),
            )
            .unwrap();

        graph.add_edge("path_a", END).unwrap();
        graph.add_edge("path_b", END).unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.has_node("router"));
        assert!(compiled.has_node("path_a"));
        assert!(compiled.has_node("path_b"));
    }

    /// 测试 set_entry_point 和 set_finish_point
    #[test]
    fn test_entry_finish_points() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "data".to_string(),
            Box::new(LastValue::new("data")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        graph
            .add_node("process", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph.set_entry_point("process").unwrap();
        graph.set_finish_point("process").unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.has_node("process"));
    }
}

// ======================== 联合测试 ========================

mod integration_tests {
    use langchain_rust::{
        prompt::{PromptFromatter, PromptTemplate, TemplateFormat},
        prompt_args,
    };

    use langgraph::{
        channels::{base::Channel, LastValue},
        prelude::*,
    };
    use std::collections::HashMap;

    /// 测试：在 langgraph 节点中使用 langchain PromptTemplate
    #[test]
    fn test_langchain_prompt_in_langgraph_node() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "output".to_string(),
            Box::new(LastValue::new("output")) as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        // 创建一个节点，内部使用 langchain 的 PromptTemplate
        graph
            .add_node("formatter", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move {
                    let name = input
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let template = PromptTemplate::new(
                        format!("Hello {{name}}! Welcome to {{place}}."),
                        vec!["name".to_string(), "place".to_string()],
                        TemplateFormat::FString,
                    );
                    let args = prompt_args! {
                        "name" => name,
                        "place" => "LangGraph",
                    };
                    // PromptFromatter trait 提供了 format 方法
                    let formatted = template.format(args).unwrap();
                    Ok(JsonValue::String(formatted))
                })
            })
            .unwrap();

        graph.add_edge(START, "formatter").unwrap();
        graph.add_edge("formatter", END).unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.has_node("formatter"));
    }

    /// 测试 StateGraph 的节点名称和通道名称
    #[test]
    fn test_graph_metadata() {
        let mut channels: HashMap<String, Box<dyn Channel>> = HashMap::new();
        channels.insert(
            "value".to_string(),
            Box::new(LastValue::new("value")) as Box<dyn Channel>,
        );
        channels.insert(
            "counter".to_string(),
            Box::new(langgraph::channels::EphemeralValue::new("counter", false))
                as Box<dyn Channel>,
        );

        let mut graph = StateGraph::new(channels);

        graph
            .add_node("node1", |input: JsonValue, _config: RunnableConfig| {
                Box::pin(async move { Ok(input) })
            })
            .unwrap();

        graph.add_edge(START, "node1").unwrap();
        graph.add_edge("node1", END).unwrap();

        let compiled = graph.compile().unwrap();

        let channel_names = compiled.channel_names();
        assert!(channel_names.contains(&"value".to_string()));
        assert!(channel_names.contains(&"counter".to_string()));
        assert!(channel_names.contains(&START.to_string()));
    }
}
