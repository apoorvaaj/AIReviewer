//! The send gate.
//!
//! 1. `build_bundle` produces the EXACT bytes of every request that will be
//!    sent (the JSON bodies, URLs, destination and models) plus a SHA-256 over
//!    all of it. The bundle is displayed in full on the
//!    "What leaves your laptop" screen.
//! 2. The student presses "Send"; the UI echoes back the hash it displayed.
//! 3. `confirm` recomputes the hash from the bundle and checks it against both
//!    the stored hash and the echoed hash. Only then does it return a
//!    `ConfirmedPayload`.
//! 4. Every network-sending method of a `ReasoningEngine` requires a
//!    `&ConfirmedPayload`, and `ConfirmedPayload` has a private field, so it
//!    cannot be created anywhere except `confirm` below. Sending without
//!    confirmation therefore does not compile.
//!
//! The second-pass request is not known before the first results arrive. It
//! is built from a fixed template (shown and hashed here) plus model questions
//! and student quotes that are verified substrings of the text shown here, so
//! it contains no student text beyond what was confirmed.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::reasoning::{CheckTask, EngineInfo, ReasoningEngine};

#[derive(Debug, Clone, Serialize)]
pub struct PlannedRequest {
    pub check_id: String,
    pub criterion: String,
    pub label: String,
    /// "sections: method, variables" / "whole document" / "whole document (sections not found)"
    pub scope: String,
    pub url: String,
    /// The exact JSON body that will be sent, byte for byte.
    pub body: String,
    /// The (redacted) document text inside `body`; quotes are validated against it.
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestBundle {
    pub engine: EngineInfo,
    pub requests: Vec<PlannedRequest>,
    pub second_pass_template: String,
    pub sha256: String,
}

pub struct ConfirmedPayload {
    bundle: RequestBundle,
    _sealed: (),
}

impl ConfirmedPayload {
    pub fn bundle(&self) -> &RequestBundle {
        &self.bundle
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfirmError {
    #[error("The payload changed after it was shown to you. Nothing was sent; please review it again.")]
    HashMismatch,
}

fn compute_hash(engine: &EngineInfo, requests: &[PlannedRequest], second_pass_template: &str) -> String {
    let mut h = Sha256::new();
    h.update(serde_json::to_string(engine).expect("EngineInfo serialises"));
    h.update(b"\n");
    for r in requests {
        h.update(r.url.as_bytes());
        h.update(b"\n");
        h.update(r.body.as_bytes());
        h.update(b"\n");
    }
    h.update(second_pass_template.as_bytes());
    hex::encode(h.finalize())
}

/// Pure function: plans requests. Never touches the network.
pub fn build_bundle(engine: &dyn ReasoningEngine, tasks: &[CheckTask]) -> RequestBundle {
    let info = engine.info();
    let requests: Vec<PlannedRequest> = tasks.iter().map(|t| engine.plan_check(t)).collect();
    let second_pass_template = engine.second_pass_template();
    let sha256 = compute_hash(&info, &requests, &second_pass_template);
    RequestBundle { engine: info, requests, second_pass_template, sha256 }
}

/// The ONLY constructor of `ConfirmedPayload`. Called only from the desktop
/// shell's `confirm_and_run` command, after the student presses "Send"
/// (enforced by tests/audit.rs).
pub fn confirm(bundle: RequestBundle, displayed_hash: &str) -> Result<ConfirmedPayload, ConfirmError> {
    let recomputed = compute_hash(&bundle.engine, &bundle.requests, &bundle.second_pass_template);
    if recomputed != bundle.sha256 || recomputed != displayed_hash {
        return Err(ConfirmError::HashMismatch);
    }
    Ok(ConfirmedPayload { bundle, _sealed: () })
}
