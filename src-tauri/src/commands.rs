//! The ENTIRE surface between the UI and the core. The webview has no file,
//! network, shell or dialog permissions; it can only call these commands.
//!
//! Flow (one session at a time, held in memory only):
//!   app_info -> open_document -> apply_redaction (repeatable)
//!   -> build_payload -> confirm_and_run -> save_report / save_receipt
//!   -> reset_session
//!
//! Disk writes happen ONLY in `save_report` and `save_receipt`, and only to a
//! path the student picks in a native save dialog (enforced by tests/audit.rs).
//! No command accepts free text that reaches a prompt: identifiers and extra
//! redaction terms only ever REMOVE text from what is sent.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use coach_core::config::Config;
use coach_core::kb::KnowledgeBase;
use coach_core::parse::{self, FileKind};
use coach_core::payload::{self, RequestBundle};
use coach_core::pipeline::{self, LoadedDocument, RunOutcome, ShownItem};
use coach_core::reasoning::{EngineInfo, ReasoningEngine};
use coach_core::receipt::{self, Receipt, ReceiptInput};
use coach_core::redact::{self, Identifiers, RedactionResult, Segment, TermSummary};
use coach_core::validate::ValidatorRules;
use coach_core::{report, sections};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub kb: Result<Arc<KnowledgeBase>, String>,
    pub rules: Result<Arc<ValidatorRules>, String>,
    pub config: Config,
    pub config_error: Option<String>,
    pub engine: Arc<dyn ReasoningEngine>,
    pub session: Mutex<Session>,
}

/// Everything about the current review. Dropped by `reset_session` and on exit.
#[derive(Default)]
pub struct Session {
    document: Option<LoadedDocument>,
    redaction: Option<RedactionResult>,
    criteria: Vec<String>,
    bundle: Option<RequestBundle>,
    finished: Option<FinishedRun>,
    running: bool,
}

struct FinishedRun {
    outcome: RunOutcome,
    receipt: Receipt,
    criteria: Vec<String>,
    created_local: String,
}

type CmdResult<T> = Result<T, String>;

fn kb(state: &AppState) -> CmdResult<Arc<KnowledgeBase>> {
    state.kb.clone().map_err(|e| format!("The knowledge base could not be loaded: {e}"))
}

fn rules(state: &AppState) -> CmdResult<Arc<ValidatorRules>> {
    state.rules.clone().map_err(|e| format!("The validator rules could not be loaded: {e}"))
}

fn session(state: &AppState) -> std::sync::MutexGuard<'_, Session> {
    state.session.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------- app_info

#[derive(Serialize)]
pub struct CriterionInfo {
    id: String,
    name: String,
    summary: String,
    checks: usize,
}

#[derive(Serialize)]
pub struct AppInfo {
    app_name: &'static str,
    app_version: &'static str,
    engine: EngineInfo,
    api_key_configured: bool,
    config_error: Option<String>,
    startup_error: Option<String>,
    kb_version: String,
    kb_syllabus_year: u32,
    kb_is_draft: bool,
    kb_notice: String,
    criteria: Vec<CriterionInfo>,
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let startup_error = match (&state.kb, &state.rules) {
        (Err(e), _) => Some(format!("Knowledge base: {e}")),
        (_, Err(e)) => Some(format!("Validator rules: {e}")),
        _ => None,
    };
    let (kb_version, kb_syllabus_year, kb_is_draft, kb_notice, criteria) = match &state.kb {
        Ok(kb) => (
            kb.manifest.kb_version.clone(),
            kb.manifest.syllabus_year,
            kb.has_unreviewed_checks(),
            kb.manifest.notice.trim().to_string(),
            kb.criteria
                .iter()
                .map(|c| CriterionInfo {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    summary: c.summary.trim().to_string(),
                    checks: kb.checks_for(std::slice::from_ref(&c.id)).len(),
                })
                .collect(),
        ),
        Err(_) => (String::new(), 0, true, String::new(), Vec::new()),
    };
    Ok(AppInfo {
        app_name: coach_core::APP_NAME,
        app_version: coach_core::APP_VERSION,
        engine: state.engine.info(),
        api_key_configured: state.config.api_key().is_some(),
        config_error: state.config_error.clone(),
        startup_error,
        kb_version,
        kb_syllabus_year,
        kb_is_draft,
        kb_notice,
        criteria,
    })
}

// ----------------------------------------------------------- open_document

#[derive(Serialize)]
pub struct OpenedDocument {
    /// File name only (shown in the UI; never sent, never in the receipt).
    file_name: String,
    kind: FileKind,
    words: usize,
    paragraphs: usize,
}

/// Shows the native open dialog and reads the chosen file into memory.
/// Returns Ok(None) if the student cancels.
#[tauri::command]
pub async fn open_document(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<OpenedDocument>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose your IA draft")
        .add_filter("IA draft (.docx, .pdf)", &["docx", "pdf"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(None) };
    let path: PathBuf = picked.into_path().map_err(|e| e.to_string())?;
    let kind = parse::kind_from_path(&path).ok_or_else(|| parse::ParseError::Unsupported.to_string())?;

    let meta = std::fs::metadata(&path).map_err(|e| format!("Could not read the file: {e}"))?;
    if meta.len() as usize > parse::MAX_FILE_BYTES {
        return Err(parse::ParseError::TooLarge.to_string());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("Could not read the file: {e}"))?;
    let doc = pipeline::load_document(&bytes, kind).map_err(|e| e.to_string())?;
    drop(bytes);

    let opened = OpenedDocument {
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        kind,
        words: doc.parsed.word_count(),
        paragraphs: doc.parsed.paragraphs.len(),
    };
    *session(&state) = Session { document: Some(doc), ..Default::default() };
    Ok(Some(opened))
}

// --------------------------------------------------------- apply_redaction

#[derive(Serialize)]
pub struct RedactionView {
    segments: Vec<Segment>,
    terms: Vec<TermSummary>,
    counts_by_kind: BTreeMap<String, usize>,
    words: usize,
    /// Section ids detected in the document (helps the student see whether
    /// headings were recognised).
    sections_found: Vec<String>,
}

/// Runs local redaction with the identifiers the student typed. Can be called
/// repeatedly as the student adds terms or marks terms as "keep visible".
#[tauri::command]
pub async fn apply_redaction(state: State<'_, AppState>, identifiers: Identifiers) -> CmdResult<RedactionView> {
    let mut s = session(&state);
    if s.running {
        return Err("A review is in progress.".into());
    }
    let doc = s.document.as_ref().ok_or("Choose a document first.")?;
    let red = redact::redact_paragraphs(&doc.parsed.paragraphs, &identifiers);
    let secs = sections::split(&red.paragraphs);
    let view = RedactionView {
        segments: red.segments.clone(),
        terms: red.terms.clone(),
        counts_by_kind: red.counts_by_kind.clone(),
        words: coach_core::text::word_count(&red.full_text()),
        sections_found: secs.detected().into_iter().collect(),
    };
    s.redaction = Some(red);
    // Any earlier payload no longer matches the text; it must be rebuilt and re-shown.
    s.bundle = None;
    s.finished = None;
    Ok(view)
}

// ----------------------------------------------------------- build_payload

#[derive(Serialize)]
pub struct PayloadView {
    bundle: RequestBundle,
    total_bytes: usize,
}

/// Builds the EXACT requests for the chosen criteria. Local and pure: no
/// network. The whole bundle is returned so the UI can display every byte.
#[tauri::command]
pub async fn build_payload(state: State<'_, AppState>, criteria: Vec<String>) -> CmdResult<PayloadView> {
    let kb = kb(&state)?;
    rules(&state)?;
    let mut s = session(&state);
    if s.running {
        return Err("A review is in progress.".into());
    }
    let red = s.redaction.as_ref().ok_or("Review the redactions first.")?;
    let criteria: Vec<String> = kb
        .criteria
        .iter()
        .filter(|c| criteria.contains(&c.id))
        .map(|c| c.id.clone())
        .collect();
    if criteria.is_empty() {
        return Err("Choose at least one criterion.".into());
    }
    let secs = sections::split(&red.paragraphs);
    let tasks = pipeline::build_tasks(&kb, &secs, &criteria);
    let bundle = payload::build_bundle(state.engine.as_ref(), &tasks);
    let total_bytes = bundle.requests.iter().map(|r| r.body.len()).sum();
    s.criteria = criteria;
    s.bundle = Some(bundle.clone());
    s.finished = None;
    Ok(PayloadView { bundle, total_bytes })
}

// --------------------------------------------------------- confirm_and_run

#[derive(Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

#[derive(Serialize)]
pub struct RunView {
    items: Vec<ShownItem>,
    receipt: Receipt,
    errors: Vec<String>,
}

/// The student pressed "Send". `displayed_hash` is the SHA-256 the payload
/// screen showed; it must match the bundle exactly or nothing is sent.
#[tauri::command]
pub async fn confirm_and_run(app: AppHandle, state: State<'_, AppState>, displayed_hash: String) -> CmdResult<RunView> {
    let kb = kb(&state)?;
    let rules = rules(&state)?;
    let (bundle, criteria, original_kind, original_sha256, redactions_by_kind) = {
        let mut s = session(&state);
        if s.running {
            return Err("A review is already in progress.".into());
        }
        let bundle = s.bundle.take().ok_or("Nothing to send. Build the payload first.")?;
        let doc = s.document.as_ref().ok_or("Choose a document first.")?;
        let red = s.redaction.as_ref().ok_or("Review the redactions first.")?;
        let parts = (doc.parsed.kind, doc.original_sha256.clone(), red.counts_by_kind.clone());
        s.running = true;
        (bundle, s.criteria.clone(), parts.0, parts.1, parts.2)
    };

    // THE send gate. The only place a ConfirmedPayload is created.
    let confirmed = match payload::confirm(bundle, &displayed_hash) {
        Ok(c) => c,
        Err(e) => {
            session(&state).running = false;
            return Err(e.to_string());
        }
    };

    let engine = state.engine.clone();
    let concurrency = state.config.max_concurrency;
    let (kb_run, rules_run, app_run) = (kb.clone(), rules.clone(), app.clone());
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let progress = |done: usize, total: usize| {
            let _ = app_run.emit("run-progress", Progress { done, total });
        };
        let outcome = pipeline::run(&confirmed, engine.as_ref(), &kb_run, &rules_run, concurrency, &progress);
        (confirmed, outcome)
    })
    .await;

    let mut s = session(&state);
    s.running = false;
    let (confirmed, outcome) = joined.map_err(|e| format!("The review stopped unexpectedly: {e}"))?;

    let now = chrono::Utc::now();
    let receipt = receipt::build(ReceiptInput {
        created_utc: now,
        kb: &kb,
        rules: &rules,
        bundle: confirmed.bundle(),
        outcome: &outcome,
        original_kind,
        original_sha256: &original_sha256,
        redactions_by_kind: &redactions_by_kind,
        criteria: &criteria,
    });
    let view = RunView { items: outcome.items.clone(), receipt: receipt.clone(), errors: outcome.errors.clone() };
    s.finished = Some(FinishedRun {
        outcome,
        receipt,
        criteria,
        created_local: chrono::Local::now().format("%d %B %Y, %H:%M").to_string(),
    });
    Ok(view)
}

// ------------------------------------------------------------- save_report

/// Shows a save dialog and writes the feedback report (HTML). Returns the
/// chosen file name, or None if the student cancelled.
#[tauri::command]
pub async fn save_report(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let kb = kb(&state)?;
    let html = {
        let s = session(&state);
        let run = s.finished.as_ref().ok_or("There is no feedback to save yet.")?;
        report::render_html(&run.outcome, &kb, &run.criteria, &run.created_local)
    };
    let Some(path) = pick_save_path(&app, "Save feedback questions", "feedback-questions.html", "Web page", "html")? else {
        return Ok(None);
    };
    std::fs::write(&path, html).map_err(|e| format!("Could not save the report: {e}"))?;
    Ok(Some(display_name(&path)))
}

// ------------------------------------------------------------ save_receipt

/// Shows a save dialog for the receipt JSON and writes the readable HTML
/// receipt next to it (same name, .html). Neither contains document text.
#[tauri::command]
pub async fn save_receipt(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let (json, html, stamp) = {
        let s = session(&state);
        let run = s.finished.as_ref().ok_or("There is no receipt to save yet.")?;
        let stamp = run.receipt.created_utc.get(..10).unwrap_or("session").to_string();
        (receipt::to_json(&run.receipt), receipt::render_html(&run.receipt), stamp)
    };
    let default_name = format!("feedback-receipt-{stamp}.json");
    let Some(json_path) = pick_save_path(&app, "Save session receipt", &default_name, "Receipt (JSON)", "json")? else {
        return Ok(None);
    };
    let html_path = json_path.with_extension("html");
    std::fs::write(&json_path, json).map_err(|e| format!("Could not save the receipt: {e}"))?;
    std::fs::write(&html_path, html).map_err(|e| format!("Could not save the receipt: {e}"))?;
    Ok(Some(format!("{} and {}", display_name(&json_path), display_name(&html_path))))
}

// ----------------------------------------------------------- reset_session

/// Drops the document, redactions, payload and results from memory.
#[tauri::command]
pub async fn reset_session(state: State<'_, AppState>) -> CmdResult<()> {
    let mut s = session(&state);
    if s.running {
        return Err("A review is in progress.".into());
    }
    *s = Session::default();
    Ok(())
}

// ---------------------------------------------------------------- helpers

fn pick_save_path(app: &AppHandle, title: &str, default_name: &str, filter: &str, ext: &str) -> CmdResult<Option<PathBuf>> {
    let picked = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(default_name)
        .add_filter(filter, &[ext])
        .blocking_save_file();
    let Some(picked) = picked else { return Ok(None) };
    let mut path = picked.into_path().map_err(|e| e.to_string())?;
    if path.extension().is_none_or(|x| !x.eq_ignore_ascii_case(ext)) {
        path.set_extension(ext);
    }
    Ok(Some(path))
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
