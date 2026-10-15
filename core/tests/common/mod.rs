//! Shared test helpers. All documents used in tests are synthetic.
#![allow(dead_code)]

pub mod redteam_cases;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use coach_core::config::Config;
use coach_core::kb::{self, KnowledgeBase};
use coach_core::net::{HttpResponse, Transport};
use coach_core::reasoning::cloud::CloudEngine;
use coach_core::validate::ValidatorRules;
use serde_json::{json, Value};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn load_kb() -> KnowledgeBase {
    kb::load(&repo_root().join("knowledge-base/physics-ia")).expect("KB loads")
}

pub fn load_rules() -> ValidatorRules {
    ValidatorRules::load(&repo_root().join("validator-rules.yaml")).expect("rules load")
}

pub fn sample(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("samples/generated").join(name)).expect("run tools/make_samples.py")
}

pub fn all_criteria() -> Vec<String> {
    ["research_design", "data_analysis", "conclusion", "evaluation"].iter().map(|s| s.to_string()).collect()
}

/// Wrap model text the way the Messages API does.
pub fn api_response(model: &str, text: &str) -> HttpResponse {
    let body = json!({
        "id": "msg_test", "type": "message", "role": "assistant", "model": model,
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn", "usage": { "input_tokens": 1, "output_tokens": 1 }
    });
    HttpResponse { status: 200, body: body.to_string() }
}

pub struct SeenRequest {
    pub url: String,
    pub body: Value,
    pub raw_body: String,
}

type Responder = dyn Fn(&Value) -> HttpResponse + Send + Sync;

/// Records every call; answers with a closure.
pub struct MockTransport {
    pub calls: AtomicUsize,
    pub seen: Mutex<Vec<SeenRequest>>,
    responder: Box<Responder>,
}

impl MockTransport {
    pub fn new(responder: impl Fn(&Value) -> HttpResponse + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(MockTransport { calls: AtomicUsize::new(0), seen: Mutex::new(Vec::new()), responder: Box::new(responder) })
    }
    pub fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Transport for MockTransport {
    fn post_json(&self, url: &str, _headers: &[(&str, String)], body: &str) -> Result<HttpResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let v: Value = serde_json::from_str(body).expect("engine sends JSON");
        let resp = (self.responder)(&v);
        self.seen.lock().unwrap().push(SeenRequest { url: url.into(), body: v, raw_body: body.into() });
        Ok(resp)
    }
}

pub fn engine_with(transport: Arc<MockTransport>) -> CloudEngine {
    CloudEngine::new(Config::default(), transport, Some("test-key".into())).without_retry_delay()
}

/// Is this request the second pass? (model == configured second-pass model)
pub fn is_second_pass(body: &Value) -> bool {
    body["model"] == Config::default().second_pass_model
}

/// The check id and criterion a check request was planned for (pinned in the schema enums).
pub fn requested_check(body: &Value) -> (String, String) {
    let props = &body["output_config"]["format"]["schema"]["properties"]["items"]["items"]["properties"];
    (
        props["check"]["enum"][0].as_str().unwrap().to_string(),
        props["criterion"]["enum"][0].as_str().unwrap().to_string(),
    )
}

/// The student text that was sent in a check request.
pub fn sent_student_text(body: &Value) -> String {
    let t = body["messages"][0]["content"][0]["text"].as_str().unwrap();
    t.trim_start_matches("<student_text>\n").trim_end_matches("\n</student_text>").to_string()
}

pub fn verdicts_all(body: &Value, pasteable: bool) -> String {
    let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
    let n = text.matches("\"index\"").count();
    let v: Vec<Value> = (0..n).map(|i| json!({ "index": i, "pasteable": pasteable })).collect();
    json!({ "verdicts": v }).to_string()
}
