use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use database::Database;
use serde_json::json;
use tower::ServiceExt;

fn record(id: &str, text: &str) -> StoreRequest {
    StoreRequest {
        source_event_id: id.into(),
        revision: 1,
        content: text.into(),
        importance: 0.6,
        metadata: BTreeMap::new(),
    }
}
fn open_db(dir: &tempfile::TempDir) -> Database {
    Database::open(&dir.path().join("app.sqlite"), &TenantPolicy::default()).unwrap()
}

#[test]
fn deleted_sessions_release_quota_without_reusing_deleted_identity() {
    let dir = tempfile::tempdir().unwrap();
    let p = TenantPolicy {
        max_sessions: 1,
        ..TenantPolicy::default()
    };
    let mut db = Database::open(&dir.path().join("app.sqlite"), &p).unwrap();
    let first = db.open_session("app", "first", "create", &p).unwrap();
    assert!(matches!(
        db.open_session("app", "second", "create", &p),
        Err(Error::Busy)
    ));
    db.transition(&first, "delete").unwrap();
    assert!(db.open_session("app", "second", "create", &p).is_ok());
    assert!(matches!(
        db.open_session("app", "first", "resume", &p),
        Err(Error::NotFound)
    ));
}
async fn finish(db: &mut Database, tenant: &str, vector: Vec<f32>) -> Job {
    let job = db.claim(tenant).unwrap().unwrap();
    let snapshot = compute_job(job.clone(), vector, TenantPolicy::default())
        .await
        .unwrap();
    assert!(db.complete(&job, snapshot, 1).unwrap());
    job
}

#[tokio::test]
async fn duplicate_revision_restart_and_session_independence() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open_db(&dir);
    let p = TenantPolicy::default();
    let a = db.open_session("app", "a", "create", &p).unwrap();
    let b = db.open_session("app", "b", "create", &p).unwrap();
    db.accept(&a, record("turn-1", "tenant A latency budget"), &p)
        .unwrap();
    let first = finish(&mut db, "app", vec![1., 0.]).await;
    let stats = db.stats(&a).unwrap();
    assert!(
        db.accept(&a, record("turn-1", "tenant A latency budget"), &p)
            .unwrap()
            .duplicate
    );
    assert!(db.claim("app").unwrap().is_none());
    assert!(matches!(
        db.accept(&a, record("turn-1", "conflicting text"), &p),
        Err(Error::Conflict(_))
    ));
    for i in 0..20 {
        db.accept(
            &b,
            record(&format!("b-{i}"), "unrelated tenant B memories"),
            &p,
        )
        .unwrap();
        finish(&mut db, "app", vec![0., 1.]).await;
    }
    let after = db.stats(&a).unwrap();
    assert_eq!(stats.basins, after.basins);
    assert_eq!(stats.edges, after.edges);
    assert_eq!(stats.candidates_scored, after.candidates_scored);
    drop(db);
    let mut db = open_db(&dir);
    let (_, snapshot, records) = db.read(&a).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(snapshot.fragments[0].content, vec![1., 0.]);
    assert_eq!(snapshot.fragments[0].id, first.record.fragment_id);
    assert!(db.claim("app").unwrap().is_none());
    let mut revision = record("turn-1", "edited A latency budget");
    revision.revision = 2;
    db.accept(&a, revision, &p).unwrap();
    finish(&mut db, "app", vec![0.8, 0.2]).await;
    let (_, state, records) = db.read(&a).unwrap();
    assert_eq!(records[0].revision, 2);
    assert_eq!(state.nodes.len(), 1);
}

#[tokio::test]
async fn reset_delete_and_failed_old_jobs_cannot_publish_into_new_generation() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open_db(&dir);
    let p = TenantPolicy::default();
    let old = db.open_session("app", "interview", "create", &p).unwrap();
    db.accept(&old, record("turn-1", "old interview"), &p)
        .unwrap();
    let job = db.claim("app").unwrap().unwrap();
    let result = compute_job(job.clone(), vec![1., 0.], p.clone())
        .await
        .unwrap();
    db.transition(&old, "reset").unwrap();
    assert!(db.verify(&old).is_err());
    let current = db.open_session("app", "interview", "resume", &p).unwrap();
    db.accept(&current, record("turn-1", "new interview"), &p)
        .unwrap();
    let newer = db.claim("app").unwrap().unwrap();
    db.fail(&job, true).unwrap();
    assert_eq!(db.stats(&current).unwrap().processing, 1);
    assert!(!db.complete(&job, result, 1).unwrap());
    assert_eq!(db.stats(&current).unwrap().processing, 1);
    let result = compute_job(newer.clone(), vec![0., 1.], p.clone())
        .await
        .unwrap();
    assert!(db.complete(&newer, result, 1).unwrap());
    db.discard(&current, "turn-1").unwrap();
    let (_, snapshot, records) = db.read(&current).unwrap();
    assert!(records.is_empty());
    assert!(snapshot.nodes.is_empty());
    assert!(snapshot.basins.is_empty());
    assert!(db
        .accept(&current, record("turn-1", "resurrection"), &p)
        .is_err());
    db.transition(&current, "delete").unwrap();
    assert!(db.open_session("app", "interview", "create", &p).is_err());
    assert!(db.open_session("app", "interview", "resume", &p).is_err());
}

#[tokio::test]
async fn atomic_commit_fault_recovery_and_retry_keep_acknowledged_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open_db(&dir);
    let p = TenantPolicy::default();
    let scope = db.open_session("app", "interview", "create", &p).unwrap();
    db.accept(&scope, record("one", "durably accepted record"), &p)
        .unwrap();
    let job = db.claim("app").unwrap().unwrap();
    let state = compute_job(job.clone(), vec![1., 0.], p).await.unwrap();
    db.connection.execute_batch("CREATE TEMP TRIGGER reject_ready BEFORE UPDATE ON records WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'simulated crash before completion'); END;").unwrap();
    assert!(db.complete(&job, state, 1).is_err());
    assert_eq!(db.stats(&scope).unwrap().ready, 0);
    assert_eq!(db.stats(&scope).unwrap().basins, 0);
    drop(db);
    let mut db = open_db(&dir);
    assert_eq!(db.stats(&scope).unwrap().pending, 1);
    finish(&mut db, "app", vec![1., 0.]).await;
    assert_eq!(db.stats(&scope).unwrap().ready, 1);
}

#[tokio::test]
async fn fairness_capacity_and_policy_reindex_reuse_canonical_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open_db(&dir);
    let p = TenantPolicy {
        max_pending: 4,
        ..Default::default()
    };
    let a = db.open_session("app", "a", "create", &p).unwrap();
    let b = db.open_session("app", "b", "create", &p).unwrap();
    for i in 0..3 {
        db.accept(&a, record(&format!("a{i}"), "bulk import"), &p)
            .unwrap();
    }
    db.accept(&b, record("b", "interactive session"), &p)
        .unwrap();
    assert!(matches!(
        db.accept(&a, record("overflow", "extra"), &p),
        Err(Error::Busy)
    ));
    let first = finish(&mut db, "app", vec![1., 0.]).await;
    assert_eq!(first.scope.session_id, "a");
    let second = finish(&mut db, "app", vec![0., 1.]).await;
    assert_eq!(second.scope.session_id, "b");
    drop(db);
    let policy = TenantPolicy {
        version: 2,
        max_connections: 8,
        ..Default::default()
    };
    let mut db = Database::open(&dir.path().join("app.sqlite"), &policy).unwrap();
    let mut reused = 0;
    while let Some(job) = db.claim("app").unwrap() {
        if job.embedding.is_some() {
            reused += 1;
        }
        db.fail(&job, false).unwrap();
    }
    assert_eq!(reused, 2);
}

async fn call(
    app: Router,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(json!(null)),
    )
}

#[tokio::test]
async fn authenticated_api_rejects_all_legacy_surfaces_and_foreign_selectors() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = uuid::Uuid::new_v4().simple().to_string();
    let operator = format!("CN_TEST_OP_{prefix}");
    let alpha = format!("CN_TEST_A_{prefix}");
    let beta = format!("CN_TEST_B_{prefix}");
    std::env::set_var(&operator, "test-operator-credential-32-characters");
    std::env::set_var(&alpha, "test-alpha-credential-32-characters");
    std::env::set_var(&beta, "test-beta-credential-32-characters");
    let registry = Registry::open(
        Settings {
            data_dir: dir.path().to_owned(),
            operator_token_env: operator.clone(),
            tenants: vec![
                TenantRegistration {
                    id: "alpha".into(),
                    token_env: alpha.clone(),
                    policy: Default::default(),
                },
                TenantRegistration {
                    id: "beta".into(),
                    token_env: beta.clone(),
                    policy: Default::default(),
                },
            ],
        },
        EmbeddingService::new(crate::config::EmbeddingServicesConfig::default()).unwrap(),
    )
    .unwrap();
    let services = crate::services::ContextNestServices::new_default()
        .await
        .unwrap();
    let app = crate::api::simple::create_simple_app_with_tenants(services, Some(registry.clone()))
        .await
        .unwrap();
    let app = crate::api::middleware::apply_middleware(app);
    assert_eq!(
        call(
            app.clone(),
            "/api/v2/sessions",
            None,
            json!({"session_id":"same","mode":"create"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, a) = call(
        app.clone(),
        "/api/v2/sessions",
        Some("test-alpha-credential-32-characters"),
        json!({"session_id":"same","mode":"create"}),
    )
    .await;
    let (_, b) = call(
        app.clone(),
        "/api/v2/sessions",
        Some("test-beta-credential-32-characters"),
        json!({"session_id":"same","mode":"create"}),
    )
    .await;
    let token = a["session_token"].as_str().unwrap();
    assert_ne!(a["session_token"], b["session_token"]);
    for surface in [
        "tools/store",
        "tools/retrieve",
        "tools/update",
        "tools/discard",
        "tools/summarize",
        "tools/reconstruct",
        "tools/resonate",
        "sessions",
        "features",
        "inbox",
        "field",
        "stats",
        "prompt-context",
        "cc/hook/stop",
        "coord/leases",
        "substrate/health",
        "llm/v1/chat/completions",
    ] {
        assert_eq!(
            call(
                app.clone(),
                &format!("/api/v1/{surface}"),
                Some(token),
                json!({})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED,
            "{surface}"
        );
    }
    for selector in [
        json!({"query":"secret","session_id":"foreign"}),
        json!({"query":"secret","session_ids":["foreign"]}),
        json!({"query":"secret","tenant_id":"beta"}),
    ] {
        assert_eq!(
            call(
                app.clone(),
                "/api/v2/memory/retrieve",
                Some(token),
                selector
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (status, _) = call(
        app.clone(),
        "/api/v2/memory/store",
        Some(token),
        json!({"source_event_id":"turn-1","content":"latency budget for alpha only"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let tenant = registry.tenants["alpha"].clone();
    let job = tenant
        .with_db(|db| db.claim("alpha"))
        .await
        .unwrap()
        .unwrap();
    registry.process_job(tenant, job).await;
    let (status, hits) = call(
        app.clone(),
        "/api/v2/memory/retrieve",
        Some(token),
        json!({"query":"latency budget"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1);
    let (_, hits) = call(
        app.clone(),
        "/api/v2/memory/retrieve",
        b["session_token"].as_str(),
        json!({"query":"latency budget"}),
    )
    .await;
    assert!(hits["hits"].as_array().unwrap().is_empty());
    let mut forged = token.to_string();
    forged.push('x');
    assert!(registry.authenticate_session(&forged).await.is_err());
    std::env::remove_var(operator);
    std::env::remove_var(alpha);
    std::env::remove_var(beta);
}
