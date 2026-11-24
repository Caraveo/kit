use super::*;

#[test]
fn splits_replies_into_text_and_checked_snippets() {
    let reply = "Woof! Here you go:\n\n```x\nTAKE Transform\n\nCREATE SpinningCoin[\n    EDIT FLOAT Speed: 90.0\n\n    VOID CREATE Update(FLOAT dt)[\n        Transform.Rotate(0, Speed * dt, 0)\n    ]\n]\n```\n\n- Attach it to the coin.\n\n```kdiagram\n{\"version\":1,\"nodes\":[{\"id\":1,\"kind\":{\"type\":\"OnStart\"},\"pos\":[0,0],\"inputs\":{},\"target\":\"SelfObject\"},{\"id\":2,\"kind\":{\"type\":\"SetTint\"},\"pos\":[320,0],\"inputs\":{\"color\":{\"Color\":[0.0,1.0,0.0,1.0]}},\"target\":\"SelfObject\"}],\"links\":[{\"from\":{\"node\":1,\"pin\":\"then\"},\"to\":{\"node\":2,\"pin\":\"exec\"}}],\"variables\":[],\"next_id\":2}\n```";
    let p = parts(reply);
    assert_eq!(p.len(), 4, "{p:?}");
    assert!(matches!(&p[0], Part::Text(t) if t.starts_with("Woof")));
    let s = snippets(reply);
    assert_eq!(s.iter().map(|(k, _)| *k).collect::<Vec<_>>(), [SnippetKind::Script, SnippetKind::Diagram]);
    assert_eq!(check_script(&s[0].1).unwrap().as_deref(), Some("SpinningCoin"));
    assert_eq!(script_file_stem("SpinningCoin"), "spinning_coin");
    let g = check_diagram(&s[1].1, &Default::default()).unwrap();
    assert_eq!(g.nodes.len(), 2);
}

#[test]
fn reports_broken_snippets_with_reasons() {
    assert!(check_script("CREATE Broken[\n    VOID CREATE Update(FLOAT dt)[\n]\n").is_err());
    let bad = r#"{"version":1,"nodes":[{"id":1,"kind":{"type":"SetTint"},"pos":[0,0],"inputs":{},"target":"SelfObject"}],"links":[{"from":{"node":9,"pin":"then"},"to":{"node":1,"pin":"exec"}}],"variables":[],"next_id":1}"#;
    let err = check_diagram(bad, &Default::default()).unwrap_err();
    assert!(err.contains("no longer exists") || err.contains("wire"), "{err}");
    assert!(check_diagram("{ not json", &Default::default()).unwrap_err().starts_with("Not a valid Diagram"));
}

#[test]
fn unfinished_code_while_streaming_is_still_code() {
    let p = parts("Here:\n```x\nCREATE A[");
    assert!(matches!(&p[1], Part::Code { kind: SnippetKind::Script, code, .. } if code == "CREATE A["));
}

#[test]
fn parses_the_stream_protocol() {
    assert_eq!(
        parse_stream_event(r#"{"start":true,"mode":"fetch","mood":"fetch","model":"Code"}"#).unwrap().unwrap(),
        ChatEvent::Start { mode: "fetch".into(), mood: "fetch".into(), model: "Code".into() }
    );
    assert_eq!(parse_stream_event(r#"{"delta":"Woof"}"#).unwrap().unwrap(), ChatEvent::Delta("Woof".into()));
    let done = parse_stream_event(r#"{"done":true,"credits":2,"balance":{"plan":"free","monthly":20,"topup":0,"total":20,"periodEnd":5}}"#).unwrap().unwrap();
    assert!(matches!(done, ChatEvent::Done { credits: 2, ref balance } if balance.total == 20));
    let err = parse_stream_event(r#"{"error":{"code":"ai_interrupted","message":"Rex lost the thread."}}"#).unwrap().unwrap_err();
    assert_eq!(err.code, "ai_interrupted");
    assert!(parse_stream_event("not json").is_none());
}

#[test]
fn tokens_are_stored_privately_and_validated() {
    let dir = std::env::temp_dir().join(format!("kit_rex_tok_{}", std::process::id()));
    let store = TokenStore::new(dir.join("rex_token"));
    assert!(store.load().is_none());
    let tok = format!("kit_{}", "a".repeat(40));
    store.save(&tok).unwrap();
    assert_eq!(store.load().as_deref(), Some(tok.as_str()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    std::fs::write(&store.path, "garbage").unwrap();
    assert!(store.load().is_none(), "malformed tokens are ignored");
    store.clear();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn context_is_trimmed_to_size() {
    assert!(context_block(&[("Scene", "  ".into())], 100).is_none());
    let c = context_block(&[("Scene", "Cube, Camera".into()), ("Errors", "x".repeat(500))], 60).unwrap();
    assert!(c.starts_with("## Scene\nCube, Camera") && c.len() <= 64, "{c}");
}

/// Talks to the real service; run with `KIT_REX_LIVE=1 cargo test -p kit_rex -- --ignored`.
#[test]
#[ignore]
fn live_service_answers() {
    if std::env::var("KIT_REX_LIVE").is_err() {
        return;
    }
    let c = Client::new(api_base(), None);
    let modes = c.modes().unwrap();
    assert_eq!(modes.len(), 6);
    let d = c.start_device("kit_rex live test").unwrap();
    assert_eq!(d.user_code.len(), 9);
    assert_eq!(c.poll_device(&d.device_code).unwrap(), DevicePoll::Pending);
    assert_eq!(c.me().unwrap_err().code, "unauthorized");
}
