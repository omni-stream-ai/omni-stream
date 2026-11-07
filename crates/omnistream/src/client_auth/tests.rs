use super::*;
use store::{random, Store};

fn context(peer: &str, origin: Option<&str>, token: Option<&str>) -> Context {
    let mut headers = "GET / HTTP/1.1\r\nHost: 127.0.0.1:4317".to_string();
    if let Some(origin) = origin {
        headers.push_str(&format!("\r\nOrigin: {origin}"));
    }
    if let Some(token) = token {
        headers.push_str(&format!("\r\nAuthorization: Bearer {token}"));
    }
    Context::parse(peer.parse().unwrap(), &headers).unwrap()
}
fn status(result: Gate) -> u16 {
    match result {
        Gate::Response(status, _) => status,
        Gate::Allow(_) => 200,
    }
}

#[test]
fn loopback_trust_requires_local_origin_host_and_no_forwarding_and_can_be_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let access = Access::new(&path, false, None).unwrap();
    for origin in [
        None,
        Some("http://localhost:1420"),
        Some("tauri://localhost"),
        Some("http://tauri.localhost"),
    ] {
        assert!(access
            .principal(&context("127.0.0.1:1", origin, None))
            .unwrap()
            .is_some());
    }
    for origin in [
        "https://evil.test",
        "null",
        "http://127.0.0.1.evil.test",
        "http://localhost@evil.test",
    ] {
        assert!(access
            .principal(&context("127.0.0.1:1", Some(origin), None))
            .unwrap()
            .is_none());
    }
    assert!(access
        .principal(&context("192.0.2.1:1", Some("http://localhost"), None))
        .unwrap()
        .is_none());
    for header in [
        "X-Forwarded-For: 127.0.0.1",
        "Forwarded: for=127.0.0.1",
        "X-Real-IP: 127.0.0.1",
    ] {
        let context = Context::parse(
            "127.0.0.1:1".parse().unwrap(),
            &format!("GET / HTTP/1.1\r\nHost: localhost\r\n{header}"),
        )
        .unwrap();
        assert!(access.principal(&context).unwrap().is_none());
    }
    let context = Context::parse(
        "127.0.0.1:1".parse().unwrap(),
        "GET / HTTP/1.1\r\nHost: public.test",
    )
    .unwrap();
    assert!(access.principal(&context).unwrap().is_none());
    assert!(Context::parse(
        "127.0.0.1:1".parse().unwrap(),
        "GET / HTTP/1.1\r\nHost: localhost\r\nHost: public.test"
    )
    .is_err());
    let required = Access::new(&path, true, None).unwrap();
    assert!(required.principal(&context_for_local()).unwrap().is_none());
}
fn context_for_local() -> Context {
    context("127.0.0.1:1", None, None)
}

#[test]
fn pending_credentials_cannot_manage_requests_or_read_business_data_and_never_leak() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let access = Access::new(&path, true, None).unwrap();
    let token = random(32).unwrap();
    let other = random(32).unwrap();
    let record = Store::open(&path)
        .unwrap()
        .request("Phone", "Android", "192.0.2.1", "https://app.test", &token)
        .unwrap();
    let owner = context("192.0.2.1:1", Some("https://app.test"), Some(&token));
    for (method, path) in [
        ("GET", "/projects"),
        ("GET", "/client-auth/requests"),
        ("GET", "/events"),
        ("POST", "/chat/stream"),
        ("GET", "/push/status"),
    ] {
        assert_eq!(status(access.gate(&owner, method, path, b"{}")), 401);
    }
    let route = format!("/client-auth/requests/{}", record.id);
    assert_eq!(status(access.gate(&owner, "GET", &route, b"")), 200);
    assert_eq!(
        status(access.gate(
            &context("192.0.2.1:1", None, Some(&other)),
            "GET",
            &route,
            b""
        )),
        404
    );
    assert_eq!(
        status(access.gate(&context("192.0.2.1:1", None, None), "GET", &route, b"")),
        404
    );
    let public = serde_json::to_string(&record).unwrap();
    assert!(!public.contains(&token) && !public.contains("token_hash"));
    let saved: String = Store::open(&path)
        .unwrap()
        .0
        .query_row("SELECT token_hash FROM client_authorizations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_ne!(saved, token);
}

#[test]
fn approval_persists_revocation_is_immediate_and_a_name_or_id_cannot_recover_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let token = random(32).unwrap();
    let mut store = Store::open(&path).unwrap();
    let first = store
        .request("Phone", "Android", "192.0.2.1", "", &token)
        .unwrap();
    assert!(store.authorized(&token).unwrap().is_none());
    assert!(store.decide(&first.id, "wrong", true, "local").is_err());
    store.decide(&first.id, &first.code, true, "local").unwrap();
    let access = Access::new(&path, true, None).unwrap();
    let caller = context("192.0.2.1:1", None, Some(&token));
    let principal = access.principal(&caller).unwrap().unwrap();
    assert_eq!(principal.client_id(), Some(first.id.as_str()));
    assert!(Store::open(&path)
        .unwrap()
        .authorized(&token)
        .unwrap()
        .is_some());
    for token in [&first.id, &first.code] {
        assert!(store.authorized(token).unwrap().is_none());
    }
    let next = store
        .request("Phone", "Android", "192.0.2.1", "", &random(32).unwrap())
        .unwrap();
    assert_eq!(next.status, "pending");
    assert_ne!(next.id, first.id);
    store.revoke(&first.id, "local").unwrap();
    assert!(!access.still_authorized(&principal));
    assert_eq!(status(access.gate(&caller, "GET", "/projects", b"")), 401);
    assert!(store.decide(&next.id, &next.code, true, &first.id).is_err());
    assert_eq!(
        store.own(&first.id, &token).unwrap().unwrap().status,
        "revoked"
    );
}

#[test]
fn request_retries_are_idempotent_and_expiry_cancellation_and_rate_limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut store = Store::open(&path).unwrap();
    let token = random(32).unwrap();
    let first = store
        .request("Phone", "Web", "192.0.2.1", "", &token)
        .unwrap();
    assert_eq!(
        store
            .request("Phone", "Web", "192.0.2.1", "", &token)
            .unwrap()
            .id,
        first.id
    );
    store.cancel(&first.id, &token).unwrap();
    assert!(store.decide(&first.id, &first.code, true, "local").is_err());
    let expired = store
        .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
        .unwrap();
    store
        .0
        .execute(
            "UPDATE client_authorizations SET expires_at=0 WHERE id=?1",
            [&expired.id],
        )
        .unwrap();
    assert_eq!(store.get(&expired.id).unwrap().unwrap().status, "expired");
    assert!(store
        .decide(&expired.id, &expired.code, true, "local")
        .is_err());
    for _ in 0..3 {
        store
            .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
            .unwrap();
    }
    assert!(store
        .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
        .unwrap_err()
        .contains("频繁"));
}

#[test]
fn competing_approvers_can_only_resolve_a_request_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let first = Store::open(&path)
        .unwrap()
        .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = [true, false].map(|approve| {
        let path = path.clone();
        let first = first.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut store = Store::open(&path).unwrap();
            barrier.wait();
            store
                .decide(&first.id, &first.code, approve, "local")
                .is_ok()
        })
    });
    barrier.wait();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>(),
        1
    );
}

#[test]
fn revoking_a_client_disables_its_push_devices_in_the_same_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    crate::storage::Store::open(&path).unwrap();
    let mut store = Store::open(&path).unwrap();
    let first = store
        .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
        .unwrap();
    store.decide(&first.id, &first.code, true, "local").unwrap();
    store.0.execute("INSERT INTO push_devices(id,secret_hash,transport,target,destination,server,locale,cursor,instance) VALUES('fixture-device','hash','web','{}','destination','https://core.test','zh-CN',0,'instance')",[]).unwrap();
    store
        .0
        .execute(
            "INSERT INTO client_auth_push_devices VALUES('fixture-device',?1)",
            [&first.id],
        )
        .unwrap();
    store.revoke(&first.id, "local").unwrap();
    let state: String = store
        .0
        .query_row(
            "SELECT state FROM push_devices WHERE id='fixture-device'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "disabled");
    assert!(crate::push::api_with_client(
        &path,
        &crate::push::Settings::default(),
        "POST",
        "/push/devices/retry",
        b"{}",
        Some(&first.id)
    )
    .is_err());
}

#[test]
fn approved_push_registration_tracks_its_owner_and_old_requests_cannot_reactivate_after_revoke() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    crate::storage::Store::open(&path).unwrap();
    let mut store = Store::open(&path).unwrap();
    let first = store
        .request("Phone", "Web", "192.0.2.1", "", &random(32).unwrap())
        .unwrap();
    store.decide(&first.id, &first.code, true, "local").unwrap();
    let input = json!({"id":"device-1234567890","secret":random(32).unwrap(),"server":"https://core.example","transport":"web","activate":true,
        "target":{"endpoint":"https://fcm.googleapis.com/fixture","keys":{"p256dh":URL_SAFE_NO_PAD.encode([4u8;65]),"auth":URL_SAFE_NO_PAD.encode([1u8;16])}}}).to_string();
    let register = || {
        crate::push::api_with_client(
            &path,
            &crate::push::Settings::default(),
            "POST",
            "/push/devices/register",
            input.as_bytes(),
            Some(&first.id),
        )
    };
    assert_eq!(register().unwrap()["state"], "active");
    let owner: String = store
        .0
        .query_row(
            "SELECT client_id FROM client_auth_push_devices WHERE device_id='device-1234567890'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner, first.id);
    store.revoke(&first.id, "local").unwrap();
    assert!(register().unwrap_err().contains("撤销"));
    let state: String = store
        .0
        .query_row(
            "SELECT state FROM push_devices WHERE id='device-1234567890'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "disabled");
}

#[test]
fn desktop_runtime_key_is_required_for_lan_mode_and_is_not_a_public_client_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let token = random(32).unwrap();
    let access = Access::new(&path, true, Some(token.clone())).unwrap();
    assert!(access.principal(&context_for_local()).unwrap().is_none());
    let caller = context("127.0.0.1:1", Some("tauri://localhost"), Some(&token));
    assert!(matches!(
        access.principal(&caller).unwrap(),
        Some(Principal::Local)
    ));
    assert!(Store::open(&path).unwrap().list(false).unwrap().is_empty());
    assert!(Store::open(&path)
        .unwrap()
        .authorized(&token)
        .unwrap()
        .is_none());
}
