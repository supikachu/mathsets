//! T1.4 装配器 DB 集成测试：批量取题 / 可见性过滤 / 题号重排 / 选项与答案装配。
//!
//! 复用 tests/api.rs 的测试库约定：未设置 `DATABASE_URL_TEST` 时跳过。

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use mathset::auth::jwt::verify_token;
use mathset::auth::middleware::AuthUser;
use mathset::build_app;
use mathset::db;
use mathset::export::assembler::assemble_exam;
use mathset::export::model::ExamRequest;
use mathset::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn create_test_app_with_pool() -> Option<(axum::Router, sqlx::PgPool)> {
    let database_url = mathset::testing::database_url()?;
    let pool = db::create_pool(&database_url, 5).await;
    db::run_migrations(&pool).await;
    let state = AppState::new(
        pool.clone(),
        "test-secret-for-integration-tests".to_string(),
        24,
        mathset::config::AiConfig::from_env(),
        "./uploads".to_string(),
    );
    Some((build_app(state), pool))
}

async fn request_json(
    app: &mut axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {}", t));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    let req = builder
        .body(match body {
            Some(b) => Body::from(serde_json::to_vec(&b).unwrap()),
            None => Body::empty(),
        })
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(json!({"error": "parse failed"}));
    (status, json)
}

async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("exp_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let email = format!("{}@test.com", username);
    let _ = request_json(
        app,
        Method::POST,
        "/api/v1/auth/register",
        Some(json!({
            "username": username,
            "email": email,
            "password": "test123",
            "display_name": "导出测试用户"
        })),
        None,
    )
    .await;
    let (_, body) = request_json(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "username": username, "password": "test123" })),
        None,
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

fn auth_user_from_token(token: &str) -> AuthUser {
    let claims = verify_token(token, "test-secret-for-integration-tests").unwrap();
    AuthUser {
        id: claims.sub,
        username: claims.username,
        role: claims.role,
        global_role: claims.global_role,
    }
}

async fn create_question(app: &mut axum::Router, token: &str, body: Value) -> Uuid {
    let (status, resp) = request_json(
        app,
        Method::POST,
        "/api/v1/questions",
        Some(body),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create question failed: {}", resp);
    resp["id"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
async fn assembler_assembles_visible_questions_and_skips_rest() {
    let Some((mut app, pool)) = create_test_app_with_pool().await else {
        eprintln!("⚠️  跳过导出装配器集成测试: DATABASE_URL_TEST 未设置");
        return;
    };

    let token_a = register_and_login(&mut app).await;
    let auth_a = auth_user_from_token(&token_a);

    // 填空题：stem 已含 ______（B2 沿用）；metadata.default_score=8
    let fill_id = create_question(
        &mut app,
        &token_a,
        json!({
            "stem": "集合 $A=\\{1,2\\}$，则 $A$ 的子集个数为______个",
            "question_type": "fill",
            "difficulty": 3,
            "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}},
            "analysis": "子集个数为 $2^n$",
            "metadata": {"default_score": 8}
        }),
    )
    .await;

    // 选择题：options 规范形；请求不给 default_score（应回退 metadata 的 8 → 此题无 metadata → 兜底 5）
    let choice_id = create_question(
        &mut app,
        &token_a,
        json!({
            "stem": "下列说法正确的是",
            "question_type": "choice",
            "difficulty": 2,
            "options": [
                {"label": "A", "content": "空集没有子集"},
                {"label": "B", "content": "空集是任何集合的子集"},
                {"label": "C", "content": "0 表示空集"},
                {"label": "D", "content": "{0} 是空集"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": ["B"]}}
        }),
    )
    .await;

    let missing = Uuid::new_v4();
    let req: ExamRequest = serde_json::from_value(json!({
        "title": "集合单元测验",
        "mode": "student",
        "sections": [
            {
                "title": "一、填空题",
                "questions": [
                    {"id": fill_id, "default_score": 3},
                    {"id": missing}
                ]
            },
            {
                "title": "二、选择题",
                "questions": [{"id": choice_id}]
            }
        ]
    }))
    .unwrap();

    let out = assemble_exam(&pool, &auth_a, &req).await.unwrap();

    // missing → 卷级警告；两道可见题连续编号 1、2
    assert_eq!(out.issues.len(), 1);
    assert!(out.issues[0].reason.contains("不存在"));
    assert_eq!(out.bundle.sections.len(), 2);

    let fill = &out.bundle.sections[0].questions[0];
    assert_eq!(fill.number, 1);
    assert_eq!(fill.kind, mathset::export::model::QuestionKind::Fill);
    assert_eq!(fill.score, 3.0);
    assert_eq!(fill.answers, vec!["4"]);
    // stem 已含 ______ → 不挖空，公式节点切分生效
    assert!(fill
        .stem
        .iter()
        .any(|n| matches!(n, mathset::export::model::InlineNode::Text { text } if text.contains("______"))));
    assert!(fill
        .stem
        .iter()
        .any(|n| matches!(n, mathset::export::model::InlineNode::Math { latex, .. } if latex.contains("1,2"))));

    let choice = &out.bundle.sections[1].questions[0];
    assert_eq!(choice.number, 2);
    assert_eq!(choice.options.len(), 4);
    assert_eq!(choice.options[1].label, "B");
    assert_eq!(choice.answers, vec!["B"]);
    // 无 default_score / 无 metadata → 兜底 5 分
    assert_eq!(choice.score, 5.0);

    // ── 不可见路径：用户 B 无法访问 A 的个人空间题目 ──
    let token_b = register_and_login(&mut app).await;
    let auth_b = auth_user_from_token(&token_b);
    let out_b = assemble_exam(&pool, &auth_b, &req).await.unwrap();
    assert!(out_b
        .bundle
        .sections
        .iter()
        .all(|s| s.questions.is_empty()));
    // fill + choice 不可见，missing 不存在 → 3 条警告
    assert_eq!(out_b.issues.len(), 3);
    assert!(out_b
        .issues
        .iter()
        .filter(|i| i.reason.contains("无权查看"))
        .count()
        >= 2);

    pool.close().await;
}
