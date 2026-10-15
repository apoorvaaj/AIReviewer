//! IB Feedback Coach — trust-critical core.
//!
//! Reading guide for reviewers (in data-flow order):
//!   parse      -> extract body text from .docx / .pdf (headers, footers, metadata, images are never read)
//!   sections   -> label paragraphs with IA sections (method, analysis, ...)
//!   redact     -> remove identifying details locally
//!   kb         -> load the human-editable knowledge base
//!   payload    -> build the EXACT requests; `ConfirmedPayload` is the send gate
//!   net        -> the only module that can open a network connection
//!   reasoning  -> the `ReasoningEngine` interface + the Anthropic cloud implementation
//!   validate   -> reject anything that is not a grounded Socratic question
//!   pipeline   -> wires the above together
//!   report / receipt -> what the student may save (receipt never contains document text)
//!
//! This crate never writes to disk. Saving is done by the desktop shell, and only
//! to paths the student picks in a save dialog.

pub mod config;
pub mod kb;
pub mod net;
pub mod parse;
pub mod payload;
pub mod pipeline;
pub mod reasoning;
pub mod receipt;
pub mod redact;
pub mod report;
pub mod sections;
pub mod text;
pub mod validate;

pub const APP_NAME: &str = "IB Feedback Coach";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
