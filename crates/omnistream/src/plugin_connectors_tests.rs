use super::*;
use std::{collections::BTreeMap, thread};
use tiny_http::{Header, Response, Server};

const TOKEN: &str = "fixture-private-token";
fn transport(
    id: &str,
    check: impl FnOnce(&mut tiny_http::Request) -> Response<std::io::Cursor<Vec<u8>>> + Send + 'static,
) -> (Connector, thread::JoinHandle<()>) {
    transport_token(id, TOKEN, check)
}
fn transport_token(
    id: &str,
    token: &'static str,
    check: impl FnOnce(&mut tiny_http::Request) -> Response<std::io::Cursor<Vec<u8>>> + Send + 'static,
) -> (Connector, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = server.server_addr();
    let prefix = match id {
        "slack" => "/api",
        "notion" | "figma" => "/v1",
        _ => "",
    };
    let client = Connector::with_base(id, token, &format!("http://{address}{prefix}")).unwrap();
    let id = id.to_owned();
    let worker = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("expected an upstream request");
        let header = |name: &'static str| {
            request
                .headers()
                .iter()
                .find(|header| header.field.equiv(name))
                .map(|header| header.value.as_str())
        };
        if id == "figma" {
            assert_eq!(header("X-Figma-Token"), Some(token));
            assert!(header("Authorization").is_none());
        } else {
            assert_eq!(
                header("Authorization"),
                Some(format!("Bearer {token}").as_str())
            );
        }
        if id == "notion" {
            assert_eq!(header("Notion-Version"), Some("2026-03-11"));
        }
        if id == "github" {
            assert_eq!(header("X-GitHub-Api-Version"), Some("2026-03-10"));
        }
        let response = check(&mut request);
        request.respond(response).unwrap();
    });
    (client, worker)
}
fn response(value: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(value.to_string())
}

#[test]
fn regression_delivery_responses_require_valid_success_evidence() {
    let cases = vec![
        (
            "slack",
            "send_message",
            json!({"channel":"C123","text":"report"}),
            vec![
                json!({}),
                json!({"ok":"false","error":"invalid_auth"}),
                json!({"ok":true}),
                json!({"ok":true,"channel":"C123","ts":17}),
            ],
        ),
        (
            "github",
            "create_issue",
            json!({"owner":"org","repo":"repo","title":"report"}),
            vec![
                json!({}),
                json!({"id":1}),
                json!({"id":1,"number":"42"}),
                json!({"id":0,"number":42}),
            ],
        ),
        (
            "notion",
            "create_page",
            json!({"parent_page_id":"parent","title":"report"}),
            vec![
                json!({}),
                json!({"id":"created","object":"error"}),
                json!({"id":"","object":"page"}),
            ],
        ),
        (
            "figma",
            "post_comment",
            json!({"file_key":"file","message":"report"}),
            vec![json!({}), json!({"id":17}), json!({"id":" "})],
        ),
    ];
    for (plugin, action, input, bodies) in cases {
        for body in bodies {
            let (connector, worker) = transport(plugin, move |_| response(body));
            let result = connector.invoke(action, &input);
            worker.join().unwrap();
            let error = result.expect_err("malformed success response must remain unknown");
            assert!(!error.definitely_not_applied, "{plugin}: {error:?}");
            assert!(error.retry_after.is_none());
        }
    }
}

#[test]
fn slack_user_mode_verifies_a_user_identity_and_rejects_bot_or_cli_tokens() {
    for (identity, valid) in [
        (
            json!({"ok":true,"user":"personal-user","user_id":"U123"}),
            true,
        ),
        (
            json!({"ok":true,"user":"bot","user_id":"U123","bot_id":"B123"}),
            false,
        ),
        (json!({"ok":true,"user":"missing-user-id"}), false),
    ] {
        let (connector, worker) =
            transport_token("slack", "xoxp-fixture-user-token", move |request| {
                assert_eq!(request.method().as_str(), "POST");
                assert_eq!(request.url(), "/api/auth.test");
                response(identity)
            });
        let result = connector.verify_mode(AuthMode::SlackUser);
        assert_eq!(result.is_ok(), valid);
        if valid {
            assert_eq!(result.unwrap(), "personal-user");
        }
        worker.join().unwrap();
    }
    for token in ["xoxb-bot", "xoxe-cli-service", "xapp-app", "xoxc-session"] {
        let connector = Connector::with_base("slack", token, "http://127.0.0.1:9").unwrap();
        assert!(connector
            .verify_mode(AuthMode::SlackUser)
            .unwrap_err()
            .message
            .contains("User OAuth Token"));
    }
}

#[test]
fn plugin_identity_endpoints_are_service_specific() {
    for (id, path, body, account) in [
        (
            "slack",
            "/api/auth.test",
            json!({"ok":true,"user":"assistant-bot"}),
            "assistant-bot",
        ),
        ("github", "/user", json!({"login":"octocat"}), "octocat"),
        (
            "notion",
            "/v1/users/me",
            json!({"object":"user","name":"Workspace bot"}),
            "Workspace bot",
        ),
        ("figma", "/v1/me", json!({"handle":"designer"}), "designer"),
    ] {
        let (connector, worker) = transport(id, move |request| {
            assert_eq!(
                request.method().as_str(),
                if id == "slack" { "POST" } else { "GET" }
            );
            assert_eq!(request.url(), path);
            response(body)
        });
        assert_eq!(connector.verify().unwrap(), account);
        worker.join().unwrap();
    }
}

#[test]
fn plugin_all_operations_build_expected_requests() {
    let cases = vec![
        (
            "slack",
            "list_channels",
            json!({"cursor":"next cursor","limit":5}),
            "GET",
            "/api/conversations.list",
            json!({"cursor":"next cursor","limit":"5","types":"public_channel","exclude_archived":"true"}),
            None,
        ),
        (
            "slack",
            "list_users",
            json!({}),
            "GET",
            "/api/users.list",
            json!({"limit":"100"}),
            None,
        ),
        (
            "slack",
            "list_users",
            json!({"limit":200,"cursor":"next cursor+/="}),
            "GET",
            "/api/users.list",
            json!({"limit":"200","cursor":"next cursor+/="}),
            None,
        ),
        (
            "slack",
            "user_info",
            json!({"user":"U123"}),
            "GET",
            "/api/users.info",
            json!({"user":"U123"}),
            None,
        ),
        (
            "slack",
            "user_info",
            json!({"user":"W123"}),
            "GET",
            "/api/users.info",
            json!({"user":"W123"}),
            None,
        ),
        (
            "slack",
            "channel_history",
            json!({"channel":"C123","cursor":"next"}),
            "GET",
            "/api/conversations.history",
            json!({"channel":"C123","limit":"15","cursor":"next"}),
            None,
        ),
        (
            "slack",
            "list_channels",
            json!({"types":"im"}),
            "GET",
            "/api/conversations.list",
            json!({"limit":"20","types":"im","exclude_archived":"true"}),
            None,
        ),
        (
            "slack",
            "list_channels",
            json!({"types":"private_channel"}),
            "GET",
            "/api/conversations.list",
            json!({"limit":"20","types":"private_channel","exclude_archived":"true"}),
            None,
        ),
        (
            "slack",
            "list_channels",
            json!({"types":"mpim"}),
            "GET",
            "/api/conversations.list",
            json!({"limit":"20","types":"mpim","exclude_archived":"true"}),
            None,
        ),
        (
            "slack",
            "channel_history",
            json!({"channel":"D123","oldest":"123.000001","latest":"125.000001"}),
            "GET",
            "/api/conversations.history",
            json!({"channel":"D123","limit":"15","oldest":"123.000001","latest":"125.000001"}),
            None,
        ),
        (
            "slack",
            "thread_replies",
            json!({"channel":"C123","ts":"123.456","cursor":"next"}),
            "GET",
            "/api/conversations.replies",
            json!({"channel":"C123","ts":"123.456","limit":"15","cursor":"next"}),
            None,
        ),
        (
            "slack",
            "open_conversation",
            json!({"users":"U123,U456"}),
            "POST",
            "/api/conversations.open",
            json!({}),
            Some(json!({"users":"U123,U456"})),
        ),
        (
            "slack",
            "send_message",
            json!({"channel":"C123","text":"Hello 世界","thread_ts":"123.456"}),
            "POST",
            "/api/chat.postMessage",
            json!({}),
            Some(json!({"channel":"C123","text":"Hello 世界","thread_ts":"123.456"})),
        ),
        (
            "github",
            "list_repositories",
            json!({"page":2}),
            "GET",
            "/user/repos",
            json!({"page":"2","per_page":"20"}),
            None,
        ),
        (
            "github",
            "get_repository",
            json!({"owner":"org","repo":"project"}),
            "GET",
            "/repos/org/project",
            json!({}),
            None,
        ),
        (
            "github",
            "list_issues",
            json!({"owner":"org","repo":"project"}),
            "GET",
            "/repos/org/project/issues",
            json!({"page":"1","per_page":"20","state":"open"}),
            None,
        ),
        (
            "github",
            "list_pull_requests",
            json!({"owner":"org","repo":"project","state":"all"}),
            "GET",
            "/repos/org/project/pulls",
            json!({"page":"1","per_page":"20","state":"all"}),
            None,
        ),
        (
            "github",
            "create_issue",
            json!({"owner":"org","repo":"project","title":"Fix","body":"Description"}),
            "POST",
            "/repos/org/project/issues",
            json!({}),
            Some(json!({"title":"Fix","body":"Description"})),
        ),
        (
            "notion",
            "search",
            json!({"query":"计划","start_cursor":"cursor-2"}),
            "POST",
            "/v1/search",
            json!({}),
            Some(json!({"query":"计划","start_cursor":"cursor-2","page_size":20})),
        ),
        (
            "notion",
            "get_page",
            json!({"page_id":"page-1"}),
            "GET",
            "/v1/pages/page-1",
            json!({}),
            None,
        ),
        (
            "notion",
            "list_blocks",
            json!({"block_id":"page-1","start_cursor":"cursor-2"}),
            "GET",
            "/v1/blocks/page-1/children",
            json!({"page_size":"20","start_cursor":"cursor-2"}),
            None,
        ),
        (
            "notion",
            "create_page",
            json!({"parent_page_id":"page-1","title":"日报","content":"已完成"}),
            "POST",
            "/v1/pages",
            json!({}),
            Some(
                json!({"parent":{"type":"page_id","page_id":"page-1"},"properties":{"title":{"type":"title","title":[{"type":"text","text":{"content":"日报"}}]}},"children":[{"object":"block","type":"paragraph","paragraph":{"rich_text":[{"type":"text","text":{"content":"已完成"}}]}}]}),
            ),
        ),
        (
            "figma",
            "get_file",
            json!({"file_key":"Ab123"}),
            "GET",
            "/v1/files/Ab123",
            json!({"depth":"2"}),
            None,
        ),
        (
            "figma",
            "get_comments",
            json!({"file_key":"Ab123"}),
            "GET",
            "/v1/files/Ab123/comments",
            json!({}),
            None,
        ),
        (
            "figma",
            "post_comment",
            json!({"file_key":"Ab123","message":"Looks good"}),
            "POST",
            "/v1/files/Ab123/comments",
            json!({}),
            Some(json!({"message":"Looks good","client_meta":{"x":0,"y":0}})),
        ),
        (
            "figma",
            "post_comment",
            json!({"file_key":"Ab123","message":"Reply","comment_id":"123"}),
            "POST",
            "/v1/files/Ab123/comments",
            json!({}),
            Some(json!({"message":"Reply","comment_id":"123"})),
        ),
    ];
    assert_eq!(
        ["slack", "github", "notion", "figma"]
            .iter()
            .map(|id| actions(id).len())
            .sum::<usize>(),
        19
    );
    for (id, name, input, method, path, query, body) in cases {
        let (connector, worker) = transport(id, move |request| {
            assert_eq!(request.method().as_str(), method);
            let url = Url::parse(&format!("http://localhost{}", request.url())).unwrap();
            assert_eq!(url.path(), path);
            let actual: BTreeMap<String, String> = url
                .query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let expected: BTreeMap<String, String> = query
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                .collect();
            assert_eq!(actual, expected);
            let mut text = String::new();
            request.as_reader().read_to_string(&mut text).unwrap();
            match body {
                Some(body) => assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), body),
                None => assert!(text.is_empty()),
            }
            response(match (id, name) {
                ("slack", "send_message") => json!({"ok":true,"channel":"C123","ts":"123.456"}),
                ("github", "create_issue") => json!({"id":1,"number":42}),
                ("notion", "create_page") => json!({"object":"page","id":"created-page"}),
                ("figma", "post_comment") => json!({"id":"created-comment"}),
                _ => json!({"ok":true,"name":"result"}),
            })
        });
        connector.invoke(name, &input).unwrap();
        worker.join().unwrap();
    }
}

#[test]
fn plugin_pagination_and_responses_are_redacted() {
    let (connector, worker) = transport("github", |_| {
        response(
            json!([{"name":"first","access_token":TOKEN,"description":format!("echo {TOKEN}")}]),
        )
        .with_header(
            Header::from_bytes(
                "Link",
                "<https://api.github.com/user/repos?page=2>; rel=\"next\"",
            )
            .unwrap(),
        )
    });
    let result = connector.invoke("list_repositories", &json!({})).unwrap();
    worker.join().unwrap();
    assert_eq!(result["next_page"], 2);
    assert!(!result.to_string().contains(TOKEN));
    assert!(result["data"][0].get("access_token").is_none());
    let (connector, worker) = transport("slack", |_| {
        response(json!({"ok":true,"channels":[],"response_metadata":{"next_cursor":"cursor2"}}))
    });
    assert_eq!(
        connector.invoke("list_channels", &json!({})).unwrap()["next_cursor"],
        "cursor2"
    );
    worker.join().unwrap();
    let (connector, worker) = transport("notion", |_| {
        response(json!({"results":[],"has_more":true,"next_cursor":"cursor3"}))
    });
    assert_eq!(
        connector.invoke("search", &json!({})).unwrap()["next_cursor"],
        "cursor3"
    );
    worker.join().unwrap();
}

#[test]
fn plugin_service_errors_do_not_leak_tokens_or_follow_redirects() {
    for (status, body, auth) in [
        (401, json!({"message":format!("invalid {TOKEN}")}), true),
        (429, json!({"message":"rate limited"}), false),
        (302, json!({"message":"redirect"}), false),
    ] {
        let (connector, worker) = transport("github", move |_| {
            response(body)
                .with_status_code(status)
                .with_header(Header::from_bytes("Retry-After", "60").unwrap())
                .with_header(Header::from_bytes("Location", "http://127.0.0.1:1/steal").unwrap())
        });
        let error = connector
            .invoke("list_repositories", &json!({}))
            .unwrap_err();
        worker.join().unwrap();
        assert!(!error.message.contains(TOKEN));
        assert!(error.message.contains(&status.to_string()));
        assert!(error.message.contains("Retry-After: 60"));
        assert_eq!(error.auth_failed, auth);
    }
    let (connector, worker) = transport("slack", |_| {
        response(json!({"ok":false,"error":"token_revoked"}))
    });
    assert!(connector.verify().unwrap_err().auth_failed);
    worker.join().unwrap();
    let (connector, worker) = transport("figma", |_| Response::from_string("not-json"));
    assert!(connector.verify().unwrap_err().message.contains("JSON"));
    worker.join().unwrap();
    let (connector, worker) = transport("figma", |_| {
        Response::from_string("x".repeat(4 * 1024 * 1024 + 1))
    });
    assert!(connector
        .invoke("get_file", &json!({"file_key":"file1"}))
        .unwrap_err()
        .message
        .contains("4 MiB"));
    worker.join().unwrap();
}

#[test]
fn plugin_schema_rejects_unknown_operations_and_untrusted_request_fields() {
    for (id, action, input) in [
        ("github", "delete_repository", json!({})),
        (
            "github",
            "get_repository",
            json!({"owner":"../admin","repo":"project"}),
        ),
        (
            "slack",
            "channel_history",
            json!({"channel":"C123","limit":16}),
        ),
        (
            "notion",
            "get_page",
            json!({"page_id":"page","url":"https://evil.example"}),
        ),
        ("figma", "get_file", json!({"file_key":"file","depth":11})),
        ("slack", "send_message", json!({"channel":"C123","text":""})),
        ("slack", "list_channels", json!({"types":"invalid"})),
        ("slack", "list_users", json!({"limit":0})),
        ("slack", "list_users", json!({"limit":201})),
        ("slack", "list_users", json!({"cursor":""})),
        ("slack", "list_users", json!({"url":"https://evil.example"})),
        ("slack", "user_info", json!({})),
        ("slack", "user_info", json!({"user":"D123"})),
        ("slack", "user_info", json!({"user":"C123"})),
        ("slack", "user_info", json!({"user":"U1,U2"})),
        ("slack", "user_info", json!({"user":"../admin"})),
        (
            "slack",
            "channel_history",
            json!({"channel":"D123","oldest":"yesterday"}),
        ),
        (
            "slack",
            "thread_replies",
            json!({"channel":"C123","ts":"bad"}),
        ),
        (
            "slack",
            "open_conversation",
            json!({"users":"U123; rm -rf"}),
        ),
        (
            "slack",
            "open_conversation",
            json!({"users":"U1,U2,U3,U4,U5,U6,U7,U8,U9"}),
        ),
    ] {
        assert!(validate(id, action, &input).is_err());
    }
}

#[test]
fn slack_member_queries_preserve_identity_pagination_and_permission_errors() {
    let members = json!([
        {"id":"U123","name":"junjie","real_name":"JunJie Wang",
         "profile":{"display_name":"JunJie","real_name":"JunJie Wang"},
         "deleted":false,"is_bot":false},
        {"id":"U456","real_name":"JunJie Wang","deleted":true,"is_bot":false}
    ]);
    for next in ["cursor2", ""] {
        let expected = members.clone();
        let body = json!({"ok":true,"members":members,"response_metadata":{"next_cursor":next}});
        let (connector, worker) = transport("slack", move |_| response(body));
        let result = connector.invoke("list_users", &json!({})).unwrap();
        worker.join().unwrap();
        assert_eq!(result["data"]["members"], expected);
        assert_eq!(
            result["next_cursor"],
            if next.is_empty() {
                Value::Null
            } else {
                json!(next)
            }
        );
    }
    let user = members[0].clone();
    let body = json!({"ok":true,"user":user});
    let (connector, worker) = transport("slack", move |_| response(body));
    let result = connector
        .invoke("user_info", &json!({"user":"U123"}))
        .unwrap();
    worker.join().unwrap();
    assert_eq!(result["data"]["user"], user);
    for (action, input) in [
        ("list_users", json!({})),
        ("user_info", json!({"user":"U123"})),
    ] {
        assert!(!validate("slack", action, &input).unwrap().write);
        let (connector, worker) = transport("slack", |_| {
            response(json!({"ok":false,"error":"missing_scope","needed":"users:read"}))
        });
        let error = connector.invoke(action, &input).unwrap_err();
        worker.join().unwrap();
        assert!(error.message.contains("missing_scope"));
        assert!(!error.auth_failed);
    }
}
