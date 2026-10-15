//! The reasoning layer, behind one interface.
//!
//! Phase 1 ships `cloud::CloudEngine` (Anthropic API). Phase 2 adds an offline
//! engine (llama.cpp running a local open-weight model) that implements the
//! same trait; parsing, redaction, validation, report and receipt are unchanged.

pub mod cloud;
pub mod prompts;

use serde::Serialize;

use crate::kb::Check;
use crate::payload::{ConfirmedPayload, PlannedRequest};
use crate::validate::second_pass::Verdict;
use crate::validate::FeedbackItem;

/// Engine-neutral description of one check to run.
#[derive(Debug, Clone)]
pub struct CheckTask {
    pub check: Check,
    pub criterion_name: String,
    /// Redacted, sanitised document text for this check.
    pub excerpt: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EngineInfo {
    /// e.g. "cloud-anthropic" (Phase 2: "local-llama")
    pub engine: String,
    /// Host the data goes to, or "this computer" for a local engine.
    pub destination: String,
    pub primary_model: String,
    pub second_pass_model: String,
    /// Plain-English data-handling statement shown before sending.
    pub data_handling: String,
}

#[derive(Debug, Clone)]
pub struct EngineResponse {
    /// The model's text output (expected to be JSON).
    pub text: String,
    /// The model that actually answered (from the API response).
    pub model: String,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum EngineError {
    #[error("no API key configured")]
    NotConfigured,
    #[error("network error: {0}")]
    Network(String),
    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the model declined this request")]
    Refused,
    #[error("the response was cut off")]
    Truncated,
    #[error("unexpected response: {0}")]
    Malformed(String),
}

pub trait ReasoningEngine: Send + Sync {
    fn info(&self) -> EngineInfo;

    /// Pure: produce the exact request for one check. Must not do I/O.
    fn plan_check(&self, task: &CheckTask) -> PlannedRequest;

    /// Pure: the fixed part of the second-pass request (shown and hashed).
    fn second_pass_template(&self) -> String;

    /// Send planned request `index`. Requires proof of confirmation.
    fn run_check(&self, confirmed: &ConfirmedPayload, index: usize) -> Result<EngineResponse, EngineError>;

    /// Ask the second-pass model about `items`. Requires proof of confirmation.
    fn run_second_pass(
        &self,
        confirmed: &ConfirmedPayload,
        items: &[FeedbackItem],
    ) -> Result<(Vec<Verdict>, String), EngineError>;
}
