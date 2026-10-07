//! Concord P0 integration suite — exercises the seven DoD items the
//! kickoff names:
//!
//! 1. Upsert creates + merge keeps absent fields and advances last_seen.
//! 2. live → idle → stale → ended transitions with TTL=0, plus
//!    re-PUT after DELETE resets started_at.
//! 3. Active listing omits ended/stale; ?status=all includes them;
//!    sorted by last_seen desc.
//! 4. Bind / Get / Rebind / bind-to-unknown 404.
//! 5. Mailbox: POST 404 / empty / oversize / 201; ?unacked filter;
//!    upsert response surfaces unacked_messages; ack idempotency.
//! 6. Bad id 400 on PUT (foo, loop:, bad kind).
//! 7. Drop/reopen of a file-backed store preserves principals, bindings,
//!    messages.
//!
//! Modelled on `cc_hooks_endpoint_test.rs` and `t5_frequency_boost_test.rs`.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{local_hostname, CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("simple app should build with coord_principals routes mounted");
    TestServer::new(app).expect("test server should start")
}

fn unique_id(tag: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("loop:{tag}-{nanos}")
}

// ────────────── DoD 1 — upsert creates + merge keeps absent ──────────────

#[tokio::test]
async fn dod1_first_put_creates_with_kind_prefix_and_subsequent_keeps_absent() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let server = make_server().await;
    let pid = unique_id("dod1");

    // First PUT — full body.
    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({
            "harness": "claude-code",
            "host": "matrix-host",
            "cwd": "/work",
            "pgid": 1234,
            "pids": [99],
            "tmux_pane": "main",
            "kill_recipe": ["pkill", "-P", "1234"],
            "priority": 5,
            "labels": {"role": "lead"}
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let first: Value = res.json();
    assert_eq!(first["principal"]["kind"], "loop");
    assert_eq!(first["principal"]["harness"], "claude-code");
    assert_eq!(first["principal"]["pgid"], 1234);
    assert_eq!(first["principal"]["labels"]["role"], "lead");
    assert_eq!(first["unacked_messages"], 0);
    let started_at_1 = first["principal"]["started_at"]
        .as_str()
        .unwrap()
        .to_string();
    let last_seen_1 = first["principal"]["last_seen"]
        .as_str()
        .unwrap()
        .to_string();

    // Wait so last_seen can advance past started_at and the prior
    // last_seen.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Second PUT — only cwd changes; everything else must persist.
    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({
            "cwd": "/other"
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let second: Value = res.json();
    assert_eq!(
        second["principal"]["harness"], "claude-code",
        "harness preserved"
    );
    assert_eq!(second["principal"]["host"], "matrix-host", "host preserved");
    assert_eq!(second["principal"]["pgid"], 1234, "pgid preserved");
    assert_eq!(second["principal"]["pids"][0], 99, "pids preserved");
    assert_eq!(second["principal"]["cwd"], "/other", "cwd replaced");
    assert_eq!(
        second["principal"]["labels"]["role"], "lead",
        "labels preserved"
    );
    assert_eq!(
        second["principal"]["started_at"].as_str().unwrap(),
        started_at_1,
        "started_at must NOT advance on a merge"
    );
    assert!(
        second["principal"]["last_seen"].as_str().unwrap() > last_seen_1.as_str(),
        "last_seen must advance on every PUT"
    );
}

// ────────────── DoD 2 — status transitions ──────────────

#[tokio::test]
async fn dod2_live_then_idle_then_stale_then_ended_rebirth() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // (a) runs with a generous TTL: with TTL=0 a fresh PUT is already "past
    // the TTL" by however many microseconds the response takes, so on a slow
    // CI runner it read back `idle` instead of `live` (flaky, then poisoned
    // ENV_LOCK for every later test). The TTL is read per call, so steps
    // (b)-(c) flip it to 0 and the host+pid probe decides idle vs stale.
    std::env::set_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", "60");
    let server = make_server().await;
    let pid = unique_id("dod2");
    let host = local_hostname().expect("hostname should resolve on the test host");
    let own_pid = std::process::id() as i64;

    // (a) Fresh PUT → live
    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({
            "host": host,
            "pids": [own_pid]
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal"]["status"], "live");

    // (b) After > TTL with a live pid on the same host → idle.
    std::env::set_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", "0");
    tokio::time::sleep(Duration::from_millis(60)).await;
    let res = server.get(&format!("/api/v1/coord/principals/{pid}")).await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(
        body["status"], "idle",
        "live pid + TTL=0 + same host → idle"
    );

    // (c) Replace pids with a definitely-dead pid (-1 is rejected by
    // pid_alive's guard before the kill probe runs) → stale. Using a
    // reaped `true` child was the original recipe; the OS can reuse
    // a freshly-reaped pid faster than this test reads it, so the
    // status flips to idle. -1 is deterministic and tests the same
    // branch: host matches + no live pids → stale.
    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({
            "host": host,
            "pids": [-1]
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    // Force last_seen into the past by sleeping past TTL.
    tokio::time::sleep(Duration::from_millis(60)).await;
    let res = server.get(&format!("/api/v1/coord/principals/{pid}")).await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["status"], "stale", "no live pid → stale");

    // (d) DELETE → ended.
    let res = server
        .delete(&format!("/api/v1/coord/principals/{pid}"))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["status"], "ended");
    let ended_at = body["ended_at"].as_str().unwrap().to_string();
    let started_before = body["started_at"].as_str().unwrap().to_string();

    // (e) Re-PUT clears ended_at and resets started_at.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({
            "host": host
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert!(
        body["principal"]["ended_at"].is_null(),
        "ended_at must be cleared on re-PUT"
    );
    assert!(
        body["principal"]["started_at"].as_str().unwrap() > started_before.as_str(),
        "started_at must reset after ended→alive rebirth"
    );
    // last_seen might equal started_at; both must be after the prior ended_at.
    assert!(
        body["principal"]["started_at"].as_str().unwrap() > ended_at.as_str(),
        "new started_at must be after the prior ended_at"
    );

    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
}

// ────────────── DoD 3 — listing ──────────────

#[tokio::test]
async fn dod3_listing_filters_inactive_and_sorts_descriptively() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", "0");
    let server = make_server().await;
    let host = local_hostname().expect("hostname should resolve on the test host");
    let own = std::process::id() as i64;

    // Three principals:
    //  - pid_live: own pid on this host → idle (still >TTL → idle)
    //  - pid_ended: deleted → ended
    //  - pid_stale: reaped pid → stale
    let pid_live = unique_id("dod3-live");
    let pid_ended = unique_id("dod3-ended");
    let pid_stale = unique_id("dod3-stale");

    server
        .put(&format!("/api/v1/coord/principals/{pid_live}"))
        .json(&json!({"host": host, "pids": [own]}))
        .await
        .assert_status(axum::http::StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(60)).await;

    server
        .put(&format!("/api/v1/coord/principals/{pid_ended}"))
        .json(&json!({"host": host}))
        .await
        .assert_status(axum::http::StatusCode::OK);
    server
        .delete(&format!("/api/v1/coord/principals/{pid_ended}"))
        .await
        .assert_status(axum::http::StatusCode::OK);

    let reaped: i64 = -1;
    server
        .put(&format!("/api/v1/coord/principals/{pid_stale}"))
        .json(&json!({"host": host, "pids": [reaped]}))
        .await
        .assert_status(axum::http::StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Active listing — only the live/idle one shows up.
    let res = server.get("/api/v1/coord/principals").await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let ids: Vec<&str> = body["principals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["principal_id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&pid_live.as_str()),
        "active must include live/idle"
    );
    assert!(
        !ids.contains(&pid_ended.as_str()),
        "active must exclude ended"
    );
    assert!(
        !ids.contains(&pid_stale.as_str()),
        "active must exclude stale"
    );

    // All listing — includes ended + stale.
    let res = server
        .get("/api/v1/coord/principals")
        .add_query_param("status", "all")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let ids: Vec<&str> = body["principals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["principal_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&pid_live.as_str()));
    assert!(ids.contains(&pid_ended.as_str()));
    assert!(ids.contains(&pid_stale.as_str()));
    assert_eq!(body["count"].as_u64().unwrap() as usize, ids.len());

    // last_seen desc — among our three, the live one was updated most
    // recently, then stale, then ended.
    let our_three: Vec<&Value> = body["principals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            let id = p["principal_id"].as_str().unwrap();
            id == pid_live || id == pid_stale || id == pid_ended
        })
        .collect();
    assert_eq!(our_three.len(), 3);
    let last_seens: Vec<&str> = our_three
        .iter()
        .map(|p| p["last_seen"].as_str().unwrap())
        .collect();
    let mut sorted = last_seens.clone();
    sorted.sort_by(|a, b| b.cmp(a)); // RFC3339 with micros is lexicographically sortable
    assert_eq!(last_seens, sorted, "must be sorted last_seen DESC");

    // Unknown status value → 400.
    let res = server
        .get("/api/v1/coord/principals")
        .add_query_param("status", "nope")
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);

    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
}

// ────────────── DoD 4 — bindings ──────────────

#[tokio::test]
async fn dod4_bind_get_rebind_and_bind_to_unknown_principal() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let server = make_server().await;
    let pid_a = unique_id("dod4-a");
    let pid_b = unique_id("dod4-b");
    let worker = unique_id("dod4-w");
    let unknown_pid = unique_id("dod4-unknown");

    server
        .put(&format!("/api/v1/coord/principals/{pid_a}"))
        .json(&json!({"harness": "h"}))
        .await
        .assert_status(axum::http::StatusCode::OK);
    server
        .put(&format!("/api/v1/coord/principals/{pid_b}"))
        .json(&json!({"harness": "h"}))
        .await
        .assert_status(axum::http::StatusCode::OK);

    // Bind → 200 Binding.
    let res = server
        .put(&format!("/api/v1/coord/bindings/{worker}"))
        .json(&json!({"principal_id": pid_a, "pid": 7}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["worker_id"], worker);
    assert_eq!(body["principal_id"], pid_a);
    assert_eq!(body["pid"], 7);

    // GET → returns same.
    let res = server
        .get(&format!("/api/v1/coord/bindings/{worker}"))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], pid_a);

    // Rebind → principal_id changes.
    let res = server
        .put(&format!("/api/v1/coord/bindings/{worker}"))
        .json(&json!({"principal_id": pid_b, "pid": 8}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], pid_b);
    let res = server
        .get(&format!("/api/v1/coord/bindings/{worker}"))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], pid_b);

    // Bind to unknown principal → 404.
    let res = server
        .put(&format!("/api/v1/coord/bindings/{worker}-other"))
        .json(&json!({"principal_id": unknown_pid}))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);

    // GET unknown binding → 404.
    let res = server
        .get(&format!("/api/v1/coord/bindings/no-such-worker-xyz"))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ────────────── DoD 5 — mailbox ──────────────

#[tokio::test]
async fn dod5_mailbox_404_400_ack_idempotent_unacked_count() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let server = make_server().await;
    let pid = unique_id("dod5");

    server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({"harness": "h"}))
        .await
        .assert_status(axum::http::StatusCode::OK);

    // POST to unknown principal → 404.
    let res = server
        .post("/api/v1/coord/principals/loop:no-such-principal-xyz/messages")
        .json(&json!({"from": "alice", "body": "hi"}))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);

    // Empty body → 400.
    let res = server
        .post(&format!("/api/v1/coord/principals/{pid}/messages"))
        .json(&json!({"from": "alice", "body": ""}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);

    // 8193-byte body → 400.
    let big = "x".repeat(8193);
    let res = server
        .post(&format!("/api/v1/coord/principals/{pid}/messages"))
        .json(&json!({"from": "alice", "body": big}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);

    // Valid POST → 201 with M-<n>.
    let res = server
        .post(&format!("/api/v1/coord/principals/{pid}/messages"))
        .json(&json!({"from": "alice", "body": "first"}))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = res.json();
    let msg_id = body["msg_id"].as_str().unwrap().to_string();
    assert!(
        msg_id.starts_with("M-"),
        "msg_id should be M-<n>; got {msg_id}"
    );
    assert_eq!(body["from"], "alice");
    assert_eq!(body["body"], "first");
    assert!(body["delivered_at"].is_null());
    assert!(body["acked_at"].is_null());

    // Unacked filter lists it; the upsert that follows reports
    // unacked_messages=1.
    let res = server
        .get(&format!("/api/v1/coord/principals/{pid}/messages"))
        .add_query_param("unacked", "true")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["msg_id"], msg_id);

    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["unacked_messages"], 1);

    // Ack it.
    let res = server
        .post(&format!(
            "/api/v1/coord/principals/{pid}/messages/{msg_id}/ack"
        ))
        .json(&json!({"by": "bob"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let first_ack: Value = res.json();
    let acked_at_first = first_ack["acked_at"].as_str().unwrap().to_string();
    assert!(!acked_at_first.is_empty());
    assert_eq!(first_ack["acked_by"], "bob");

    // List unacked → empty; upsert now reports unacked_messages=0.
    let res = server
        .get(&format!("/api/v1/coord/principals/{pid}/messages"))
        .add_query_param("unacked", "true")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["messages"].as_array().unwrap().len(), 0);

    let res = server
        .put(&format!("/api/v1/coord/principals/{pid}"))
        .json(&json!({}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["unacked_messages"], 0);

    // Full list still contains the acked one (oldest first).
    let res = server
        .get(&format!("/api/v1/coord/principals/{pid}/messages"))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["msg_id"], msg_id);
    assert_eq!(msgs[0]["acked_by"], "bob");

    // Second ack → same acked_at and acked_by.
    let res = server
        .post(&format!(
            "/api/v1/coord/principals/{pid}/messages/{msg_id}/ack"
        ))
        .json(&json!({"by": "carol"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let second: Value = res.json();
    assert_eq!(second["acked_at"], first_ack["acked_at"]);
    assert_eq!(
        second["acked_by"], "bob",
        "second ack must NOT overwrite by"
    );
}

// ────────────── DoD 6 — bad id 400 ──────────────

#[tokio::test]
async fn dod6_bad_ids_return_400_on_put() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let server = make_server().await;

    // `foo` — no kind prefix at all.
    let res = server
        .put("/api/v1/coord/principals/foo")
        .json(&json!({"harness": "h"}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);

    // `loop:` — prefix but empty name.
    let res = server
        .put("/api/v1/coord/principals/loop%3A")
        .json(&json!({"harness": "h"}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);

    // `bad kind:x` — invalid kind prefix.
    let res = server
        .put("/api/v1/coord/principals/bad%20kind%3Ax")
        .json(&json!({"harness": "h"}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

// ────────────── DoD 7 — drop/reopen persistence ──────────────

#[tokio::test]
async fn malformed_ids_are_404_off_the_put_route_and_bad_bodies_are_invalid_body() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let server = make_server().await;

    // A malformed id cannot name a stored principal: every non-PUT route
    // answers 404 (only the PUT upsert answers 400).
    for bad in ["foo", "loop%3A", "bad%20kind%3Ax"] {
        server
            .get(&format!("/api/v1/coord/principals/{bad}"))
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);
        server
            .delete(&format!("/api/v1/coord/principals/{bad}"))
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);
        server
            .get(&format!(
                "/api/v1/coord/principals/{bad}/messages?unacked=true"
            ))
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);
        server
            .post(&format!("/api/v1/coord/principals/{bad}/messages"))
            .json(&json!({"from": "a", "body": "x"}))
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);
        server
            .post(&format!("/api/v1/coord/principals/{bad}/messages/M-1/ack"))
            .json(&json!({"by": "a"}))
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    // An oversized body is a 400 that names the body, not the id.
    let pid = unique_id("bodycheck");
    let enc = pid.replace(':', "%3A");
    server
        .put(&format!("/api/v1/coord/principals/{enc}"))
        .json(&json!({"harness": "h"}))
        .await
        .assert_status(axum::http::StatusCode::OK);
    let res = server
        .post(&format!("/api/v1/coord/principals/{enc}/messages"))
        .json(&json!({"from": "a", "body": "x".repeat(8193)}))
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);
    let body: Value = res.json();
    let err = body["error"].as_str().unwrap_or("");
    assert!(
        err.contains("body") && !err.contains("invalid id"),
        "oversize must be reported as a body error, got {body}"
    );
}

#[tokio::test]
async fn dod7_file_backed_store_persists_across_drop() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS");
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("coord.db");
    let pid = unique_id("dod7");
    let worker = unique_id("dod7-w");

    // Open, write, bind, post, drop.
    {
        let store = CoordStore::open(&db_path).expect("open file-backed store");
        store
            .upsert_principal(
                &pid,
                PrincipalUpsert {
                    harness: Some("h".into()),
                    host: local_hostname(),
                    ..Default::default()
                },
            )
            .expect("upsert");
        store.bind(&worker, &pid, Some(11)).expect("bind");
        store.post_message(&pid, "alice", "hello").expect("post");
    }
    // Reopen and confirm everything survived.
    let store2 = CoordStore::open(&db_path).expect("reopen");
    let p = store2
        .get_principal(&pid)
        .expect("get")
        .expect("principal present");
    assert_eq!(p.harness.as_deref(), Some("h"));
    assert_eq!(
        p.host,
        local_hostname(),
        "persisted host must equal what we wrote"
    );
    let b = store2
        .get_binding(&worker)
        .expect("get binding")
        .expect("binding present");
    assert_eq!(b.principal_id, pid);
    assert_eq!(b.pid, Some(11));
    let msgs = store2.list_messages(&pid, false).expect("list messages");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "hello");
    assert_eq!(msgs[0].from, "alice");
}
