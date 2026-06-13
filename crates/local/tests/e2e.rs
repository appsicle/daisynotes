//! Optional end-to-end smoke test for the local brain.
//!
//! Skipped by default (it needs a downloaded model and real inference):
//! `DAISYNOTES_LOCAL_E2E=1 cargo test -p daisynotes-local -- --ignored`

use daisynotes_api::{ChatMessage, ClaudeRequest, Role};

#[test]
#[ignore = "needs a downloaded model; run with DAISYNOTES_LOCAL_E2E=1 -- --ignored"]
fn one_real_generation_when_a_model_is_installed() {
    if std::env::var("DAISYNOTES_LOCAL_E2E").as_deref() != Ok("1") {
        eprintln!("DAISYNOTES_LOCAL_E2E not set; skipping");
        return;
    }
    let Some(model) = daisynotes_local::installed_model() else {
        eprintln!("no model installed; skipping");
        return;
    };
    eprintln!("running e2e against {}", model.display_name());

    let handle = daisynotes_local::spawn();
    let req = ClaudeRequest {
        system: "You live in the margin of someone's notebook. Most of the time you pass."
            .to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "The entry is between the markers.\n\n<entry>\nThe harbor was \
                      empty by noon, and I still don't know why that bothered me.\n</entry>\n"
                .to_string(),
        }],
        ..ClaudeRequest::default()
    };
    let reply = futures::executor::block_on(handle.request(req))
        .expect("receiver resolves")
        .expect("inference succeeds");

    let tool = reply.tool_name.as_deref().expect("a tool call");
    assert!(
        ["pass", "leave_notes", "respond"].contains(&tool),
        "unexpected tool {tool:?}"
    );
    assert_eq!(reply.stop_reason.as_deref(), Some("tool_use"));
    assert!(reply.tool_input.is_some());
}
