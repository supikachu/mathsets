//! AI 异步解析端到端测试脚本
//!
//! 运行方式（PowerShell）：
//!     $env:DATABASE_URL="postgres://postgres@127.0.0.1/mathset"
//!     cargo run --bin test_ai_flow
//!
//! 脚本流程：
//! 1. 后台启动 Axum Web 服务 + AI Worker 协程
//! 2. 注册测试用户 → 登录获取 JWT
//! 3. 向 POST /api/v1/ai/parse 提交测试文本 → 拿到 task_id
//! 4. 每 2s 轮询 GET /api/v1/ai/parse/:id 直到 completed / failed
//! 5. 完成时调用题目详情接口打印落库结果；失败时打印 error_message

use std::time::Duration;

use mathset::build_app;
use mathset::config::AppConfig;
use mathset::db;
use mathset::workers::ai_parse_worker::start_worker;
use serde_json::{json, Value};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // 1. 加载 .env（DATABASE_URL / AI Key 等都在这里）
    let _ = dotenvy::dotenv();

    // 2. 初始化 tracing 日志（观察 Worker 的 tracing::info! 输出）
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mathset=info".into()),
        )
        .init();

    // 3. 加载配置 + 连接数据库 + 运行迁移
    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url, 5).await;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("数据库迁移失败");

    // 4. 构建共享状态
    let state = mathset::AppState::new(
        pool,
        config.jwt_secret.clone(),
        config.jwt_expiry_hours,
        config.ai.clone(),
        config.upload_dir.clone(),
    );

    // 5. 启动 AI Worker 后台协程
    tokio::spawn(start_worker(state.clone()));
    tracing::info!("🤖 AI Worker 已在后台启动");

    // 6. 构建 Axum 路由
    let app = build_app(state);

    // 7. 绑定固定端口 9527（避免与正在运行的生产服务 3000 冲突）
    let port: u16 = 9527;
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("无法绑定测试端口");

    // 8. 后台启动 Axum 服务
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("服务启动失败");
    });

    // 9. 等待服务就绪
    tokio::time::sleep(Duration::from_secs(1)).await;
    let base_url = format!("http://{}", addr);
    println!("🚀 测试服务已启动：{}", base_url);

    // ────────────────────────────────────────────────────────────────
    // 步骤 1：注册测试用户
    // ────────────────────────────────────────────────────────────────
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180)) // LLM 调用可能较慢，放宽客户端超时
        .build()
        .expect("创建 HTTP 客户端失败");

    let username = format!(
        "testai_{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );

    println!("\n━━━ 步骤 1：注册测试用户 [{}] ━━━", username);
    let resp = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "test123",
            "display_name": "AI 流程测试用户"
        }))
        .send()
        .await
        .expect("注册请求失败");
    println!("  注册响应状态: {}", resp.status());

    // ────────────────────────────────────────────────────────────────
    // 步骤 2：登录获取 JWT
    // ────────────────────────────────────────────────────────────────
    println!("\n━━━ 步骤 2：登录获取 JWT ━━━");
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&json!({"username": username, "password": "test123"}))
        .send()
        .await
        .expect("登录请求失败");
    let body: Value = resp.json().await.expect("解析登录响应失败");
    let token = body["token"]
        .as_str()
        .expect("响应中缺少 token")
        .to_string();
    println!("  ✅ 获取 Token（长度: {}）", token.len());

    // ────────────────────────────────────────────────────────────────
    // 步骤 3：提交 AI 解析任务
    // ────────────────────────────────────────────────────────────────
    println!("\n━━━ 步骤 3：提交 AI 解析任务 ━━━");
    let test_text = "已知集合 A={1, 2, 3}, B={2, 3, 4}，求 A 并 B。解析：两个集合合并去重即可。答案：{1, 2, 3, 4}";
    println!("  📝 提交文本: {}", test_text);

    let resp = client
        .post(format!("{}/api/v1/ai/parse", base_url))
        .bearer_auth(&token)
        .json(&json!({"raw_text": test_text}))
        .send()
        .await
        .expect("提交任务失败");
    let status = resp.status();
    let body: Value = resp.json().await.expect("解析任务响应失败");
    println!("  HTTP 状态: {}", status);
    println!("  响应体: {}", body);

    if status != reqwest::StatusCode::ACCEPTED {
        panic!("❌ 任务提交失败，期望 202 Accepted，实际 {}", status);
    }

    let task_id = body["task_id"]
        .as_str()
        .expect("响应中缺少 task_id")
        .to_string();
    println!("  ✅ 任务已入队，task_id = {}", task_id);

    // ────────────────────────────────────────────────────────────────
    // 步骤 4：轮询任务状态（每 2s 一次，最多 3 分钟）
    // ────────────────────────────────────────────────────────────────
    println!("\n━━━ 步骤 4：轮询任务状态 ━━━");
    let mut attempts = 0u32;
    let max_attempts = 90u32; // 90 * 2s = 3 分钟
    let mut final_body: Value = json!({});

    loop {
        attempts += 1;
        if attempts > max_attempts {
            println!("\n  ❌ 超过最大轮询次数（{} 次 = {} 分钟），退出", max_attempts, max_attempts * 2 / 60);
            std::process::exit(1);
        }

        let resp = client
            .get(format!("{}/api/v1/ai/parse/{}", base_url, task_id))
            .bearer_auth(&token)
            .send()
            .await
            .expect("查询任务失败");

        let http_status = resp.status();
        let body: Value = resp.json().await.expect("解析查询响应失败");
        let task_status = body["status"]
            .as_str()
            .unwrap_or("(unknown)")
            .to_string();

        let label = match task_status.as_str() {
            "pending" => "正在排队...",
            "processing" => "正在解析...",
            "completed" => "✅ 解析完成",
            "failed" => "❌ 解析失败",
            _ => "未知状态",
        };
        println!(
            "  [{:>3}/{:>3}] 状态: {:<12} {} [HTTP {}]",
            attempts, max_attempts, task_status, label, http_status
        );

        if task_status == "completed" || task_status == "failed" {
            final_body = body;
            break;
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // ────────────────────────────────────────────────────────────────
    // 步骤 5：展示最终结果
    // ────────────────────────────────────────────────────────────────
    if final_body["status"] == "completed" {
        // question_id 在 JSON 中是 UUID 字符串
        let question_id = final_body["question_id"]
            .as_str()
            .unwrap_or("(missing question_id)")
            .to_string();

        println!("\n━━━ 步骤 5：任务完成 ━━━");
        println!("  🎉 生成的题目 ID: {}", question_id);
        println!("  📅 创建时间: {}", final_body["created_at"]);
        println!("  🔄 更新时间: {}", final_body["updated_at"]);

        // ────────────────────────────────────────────────────────────
        // 步骤 6：调用题目详情接口打印落库结构
        // ────────────────────────────────────────────────────────────
        println!("\n━━━ 步骤 6：获取落库题目详情 ━━━");
        let resp = client
            .get(format!("{}/api/v1/questions/{}", base_url, question_id))
            .bearer_auth(&token)
            .send()
            .await
            .expect("获取题目详情失败");

        let http_status = resp.status();
        let body: Value = resp.json().await.expect("解析题目详情失败");
        println!("  HTTP 状态: {}", http_status);

        if http_status.is_success() {
            println!("\n  📝 落库的题目结构：");
            let pretty = serde_json::to_string_pretty(&body).unwrap_or_default();
            // 缩进打印
            for line in pretty.lines() {
                println!("    {}", line);
            }

            // 关键字段摘要
            println!("\n  📊 关键字段摘要：");
            println!("    - 题型: {}", body["question_type"]);
            println!("    - 难度: {}", body["difficulty"]);
            println!("    - 状态: {}", body["status"]);
            println!("    - 版本: {}", body["version"]);
            println!("    - 题干: {}",
                body["stem"]
                    .as_str()
                    .map(|s| if s.chars().count() > 80 { format!("{}...", s.chars().take(80).collect::<String>()) } else { s.to_string() })
                    .unwrap_or_else(|| "(missing)".to_string())
            );
            println!("    - 正确答案: {}", body["correct_answer"]);
            if let Some(analysis) = body["analysis"].as_str() {
                let preview: String = analysis.chars().take(120).collect();
                println!("    - 解析（前 120 字）: {}{}", preview, if analysis.chars().count() > 120 { "..." } else { "" });
            }
        } else {
            println!("  ❌ 获取题目详情失败: {}", body);
        }
    } else {
        // 任务失败
        println!("\n━━━ ❌ 任务失败 ━━━");
        let err_msg = final_body["error_message"]
            .as_str()
            .unwrap_or("(无错误信息)");
        println!("  错误详情: {}", err_msg);
        std::process::exit(2);
    }

    println!("\n✅ 端到端测试流程结束");
}
