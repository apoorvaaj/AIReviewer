//! Anthropic Messages API implementation of `ReasoningEngine`.
//!
//! Raw HTTPS (there is no official Rust SDK). Each request uses structured
//! output (`output_config.format` = json_schema), so the model can only return
//! the FeedbackItem shape; the validator re-checks everything anyway.
//!
//! Server-side model fallback is deliberately NOT enabled: the payload screen
//! names the model that will receive the data, and a silent switch would make
//! that statement untrue. The receipt records the model that actually answered.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::prompts::{self, CHECK_SYSTEM_PROMPT, SECOND_PASS_SYSTEM_PROMPT};
use super::{CheckTask, EngineError, EngineInfo, EngineResponse, ReasoningEngine};
use crate::config::Config;
use crate::net::Transport;
use crate::payload::{ConfirmedPayload, PlannedRequest};
use crate::validate::second_pass::{parse_verdicts, Verdict};
use crate::validate::FeedbackItem;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const RETRY_DELAYS_SECS: &[u64] = &[2, 6];

pub struct CloudEngine {
    cfg: Config,
    transport: Arc<dyn Transport>,
    api_key: Option<String>,
    retry_delay_scale: f64,
}

impl CloudEngine {
    pub fn new(cfg: Config, transport: Arc<dyn Transport>, api_key: Option<String>) -> Self {
        CloudEngine { cfg, transport, api_key, retry_delay_scale: 1.0 }
    }

    /// Tests: no real sleeping between retries.
    pub fn without_retry_delay(mut self) -> Self {
        self.retry_delay_scale = 0.0;
        self
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    fn url(&self) -> String {
        format!("{}/v1/messages", self.cfg.api_base_url.trim_end_matches('/'))
    }

    fn data_handling_text(&self) -> String {
        let retention = if self.cfg.zero_data_retention {
            "This app's operator has a Zero Data Retention arrangement with Anthropic, under which API inputs \
and outputs are not stored after the response is returned (except where required by law or to combat misuse)."
        } else {
            "Anthropic does not use API data to train its models by default. Under Anthropic's standard API terms, \
inputs and outputs may be kept for a limited period (for example for abuse monitoring) and then deleted. \
This app's operator has NOT confirmed a Zero Data Retention arrangement."
        };
        format!(
            "Your redacted text is sent over an encrypted connection to {} (Anthropic) to generate questions. {retention}",
            self.cfg.destination_host()
        )
    }

    /// POST with retries on rate-limit / overload / server errors.
    fn post(&self, body: &str) -> Result<Value, EngineError> {
        let key = self.api_key.as_ref().ok_or(EngineError::NotConfigured)?;
        let headers = [("x-api-key", key.clone()), ("anthropic-version", ANTHROPIC_VERSION.to_string())];
        let mut attempt = 0;
        loop {
            let result = self.transport.post_json(&self.url(), &headers, body);
            let retryable = match &result {
                Err(_) => true,
                Ok(r) => matches!(r.status, 429 | 500 | 502 | 503 | 504 | 529),
            };
            if retryable && attempt < RETRY_DELAYS_SECS.len() {
                let secs = RETRY_DELAYS_SECS[attempt] as f64 * self.retry_delay_scale;
                std::thread::sleep(Duration::from_secs_f64(secs));
                attempt += 1;
                continue;
            }
            let resp = result.map_err(EngineError::Network)?;
            let value: Value = serde_json::from_str(&resp.body)
                .map_err(|_| EngineError::Malformed(format!("HTTP {} with non-JSON body", resp.status)))?;
            if resp.status != 200 {
                let message = value["error"]["message"].as_str().unwrap_or("unknown error").to_string();
                return Err(EngineError::Api { status: resp.status, message });
            }
            return Ok(value);
        }
    }

    fn extract(value: &Value) -> Result<EngineResponse, EngineError> {
        match value["stop_reason"].as_str() {
            Some("refusal") => return Err(EngineError::Refused),
            Some("max_tokens") => return Err(EngineError::Truncated),
            _ => {}
        }
        let text: String = value["content"]
            .as_array()
            .ok_or_else(|| EngineError::Malformed("no content".into()))?
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        let model = value["model"].as_str().unwrap_or("unknown").to_string();
        Ok(EngineResponse { text, model })
    }

    fn request_body(&self, model: &str, effort: &str, system: &str, content: Value, schema: Value) -> String {
        let body = json!({
            "model": model,
            "max_tokens": self.cfg.max_tokens,
            "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
            "messages": [{ "role": "user", "content": content }],
            "output_config": {
                "effort": effort,
                "format": { "type": "json_schema", "schema": schema }
            }
        });
        serde_json::to_string_pretty(&body).expect("request serialises")
    }
}

impl ReasoningEngine for CloudEngine {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            engine: "cloud-anthropic".into(),
            destination: self.cfg.destination_host(),
            primary_model: self.cfg.primary_model.clone(),
            second_pass_model: self.cfg.second_pass_model.clone(),
            data_handling: self.data_handling_text(),
        }
    }

    fn plan_check(&self, task: &CheckTask) -> PlannedRequest {
        // Student text first (cached across checks that share the same
        // sections), then the check definition.
        let content = json!([
            { "type": "text", "text": prompts::student_text_block(task), "cache_control": { "type": "ephemeral" } },
            { "type": "text", "text": prompts::check_block(task) }
        ]);
        let schema = prompts::check_output_schema(&task.check.criterion, &task.check.id);
        PlannedRequest {
            check_id: task.check.id.clone(),
            criterion: task.check.criterion.clone(),
            label: task.check.description.trim().to_string(),
            scope: task.scope.clone(),
            url: self.url(),
            body: self.request_body(&self.cfg.primary_model, &self.cfg.effort, CHECK_SYSTEM_PROMPT, content, schema),
            excerpt: task.excerpt.clone(),
        }
    }

    fn second_pass_template(&self) -> String {
        format!(
            "Model: {}\nSystem prompt:\n{}\n\nUser message: the feedback items that passed all local checks, as\n\
<items>[{{\"index\": 0, \"student_quote\": \"<exact excerpt of the text above>\", \"question\": \"<model-written question>\"}}, ...]</items>\n\
Every student_quote is verified to be an exact excerpt of the redacted text above before this request is sent.",
            self.cfg.second_pass_model, SECOND_PASS_SYSTEM_PROMPT
        )
    }

    fn run_check(&self, confirmed: &ConfirmedPayload, index: usize) -> Result<EngineResponse, EngineError> {
        let req = confirmed
            .bundle()
            .requests
            .get(index)
            .ok_or_else(|| EngineError::Malformed("no such request".into()))?;
        let value = self.post(&req.body)?;
        Self::extract(&value)
    }

    fn run_second_pass(
        &self,
        _confirmed: &ConfirmedPayload,
        items: &[FeedbackItem],
    ) -> Result<(Vec<Verdict>, String), EngineError> {
        let content = json!([{ "type": "text", "text": prompts::second_pass_items_block(items) }]);
        let body = self.request_body(
            &self.cfg.second_pass_model,
            &self.cfg.second_pass_effort,
            SECOND_PASS_SYSTEM_PROMPT,
            content,
            prompts::second_pass_schema(),
        );
        let value = self.post(&body)?;
        let resp = Self::extract(&value)?;
        let verdicts = parse_verdicts(&resp.text).map_err(EngineError::Malformed)?;
        Ok((verdicts, resp.model))
    }
}
