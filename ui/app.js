// IB Feedback Coach UI.
//
// Rules for this file (checked by core/tests/audit.rs):
//   - no network APIs, no web storage, no HTML string injection;
//   - all text is inserted with textContent via el();
//   - the only way to reach the core is invoke() on the commands in
//     src-tauri/src/commands.rs.
// Document text lives in the Rust process; this page only displays it.

"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

/** Build an element. Children may be strings (inserted as text) or nodes. */
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (v === true) node.setAttribute(k, "");
    else if (v !== false && v != null) node.setAttribute(k, String(v));
  }
  for (const c of children.flat()) {
    if (c == null) continue;
    node.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return node;
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

const state = {
  info: null,
  ids: emptyIdentifiers(),
  payload: null,
};

function emptyIdentifiers() {
  return {
    student_names: [],
    candidate_codes: [],
    school_names: [],
    supervisor_names: [],
    extra_terms: [],
    excluded_terms: [],
  };
}

const KIND_LABELS = {
  student_name: "your name",
  candidate_code: "candidate code",
  school: "school",
  supervisor: "supervisor",
  email: "email",
  phone: "phone",
  student_added: "added by you",
};

// ------------------------------------------------------------ navigation

const SCREEN_STEP = { open: "open", redact: "redact", criteria: "criteria", payload: "payload", running: "payload", results: "results" };

function show(screen) {
  for (const s of document.querySelectorAll(".screen")) s.hidden = s.id !== `screen-${screen}`;
  for (const li of $("steps").children) li.classList.toggle("active", li.dataset.step === SCREEN_STEP[screen]);
  hideBanner();
  window.scrollTo(0, 0);
}

function showBanner(message) {
  const b = $("banner");
  b.textContent = message;
  b.hidden = false;
  window.scrollTo(0, 0);
}

function hideBanner() {
  $("banner").hidden = true;
}

/** Run a command; on failure show the message and return undefined. */
async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (e) {
    showBanner(typeof e === "string" ? e : "Something went wrong. Nothing was sent.");
    return undefined;
  }
}

async function busy(button, fn) {
  button.disabled = true;
  try {
    await fn();
  } finally {
    button.disabled = false;
  }
}

// ------------------------------------------------------------ 1. open

async function init() {
  const info = await call("app_info");
  if (!info) return;
  state.info = info;
  $("app-version").textContent =
    `${info.app_name} ${info.app_version} · knowledge base ${info.kb_version} (${info.kb_syllabus_year} syllabus) · model ${info.engine.primary_model}`;

  const warnings = $("startup-warnings");
  clear(warnings);
  if (info.startup_error) {
    warnings.append(el("p", { class: "error" }, `The app cannot start a review: ${info.startup_error}`));
    $("btn-open").disabled = true;
  }
  if (info.config_error) {
    warnings.append(el("p", { class: "warn" }, `Your config file has a problem, so default settings are used: ${info.config_error}`));
  }
  if (!info.api_key_configured) {
    warnings.append(el("p", { class: "warn" },
      "No API key is set up, so this app cannot send anything. You can still try the redaction and see what would be sent. " +
      "See the README for how to set ANTHROPIC_API_KEY."));
  }
  if (info.kb_is_draft) {
    warnings.append(el("p", { class: "note" }, `Prototype: ${info.kb_notice}`));
  }
  buildCriteriaList();
}

$("btn-open").addEventListener("click", (e) => busy(e.currentTarget, async () => {
  const opened = await call("open_document");
  if (!opened) return;
  state.ids = emptyIdentifiers();
  state.payload = null;
  $("form-ids").reset();
  $("doc-summary").textContent =
    `${opened.file_name} · ${opened.kind.toUpperCase()} · ${opened.words.toLocaleString()} words. ` +
    "Only the main text is read. Headers, footers, comments, file properties and images are ignored.";
  $("redaction-result").hidden = true;
  show("redact");
  await runRedaction();
}));

// ------------------------------------------------------------ 2. redaction

function splitEntries(value) {
  return value.split(";").map((s) => s.trim()).filter(Boolean);
}

$("form-ids").addEventListener("submit", (e) => {
  e.preventDefault();
  const f = e.currentTarget.elements;
  for (const key of ["student_names", "candidate_codes", "school_names", "supervisor_names"]) {
    state.ids[key] = splitEntries(f[key].value);
  }
  runRedaction();
});

$("form-extra").addEventListener("submit", (e) => {
  e.preventDefault();
  const input = e.currentTarget.elements.extra;
  const term = input.value.trim();
  if (!term) return;
  if (!state.ids.extra_terms.includes(term)) state.ids.extra_terms.push(term);
  state.ids.excluded_terms = state.ids.excluded_terms.filter((t) => t.toLowerCase() !== term.toLowerCase());
  input.value = "";
  runRedaction();
});

async function runRedaction() {
  const view = await call("apply_redaction", { identifiers: state.ids });
  if (!view) return;
  state.payload = null;

  // Terms that were hidden, plus terms the student chose to keep visible.
  const list = $("term-list");
  clear(list);
  for (const t of view.terms) {
    const box = el("input", { type: "checkbox", checked: true });
    box.addEventListener("change", () => toggleKeep(t.term, !box.checked));
    list.append(el("li", {}, el("label", {}, box, el("mark", {}, t.term), el("span", { class: "kind" }, `${KIND_LABELS[t.kind] || t.kind} ×${t.count}`))));
  }
  for (const term of state.ids.excluded_terms) {
    const box = el("input", { type: "checkbox" });
    box.addEventListener("change", () => toggleKeep(term, !box.checked));
    list.append(el("li", {}, el("label", {}, box, el("span", {}, term), el("span", { class: "kind" }, "kept visible"))));
  }
  if (!list.children.length) {
    list.append(el("li", { class: "muted" }, "Nothing was found to hide yet. Type your details above."));
  }

  const doc = $("redacted-text");
  clear(doc);
  for (const seg of view.segments) {
    doc.append(seg.redacted ? el("mark", { title: KIND_LABELS[seg.redacted] || "" }, seg.text) : document.createTextNode(seg.text));
  }
  const hidden = Object.values(view.counts_by_kind).reduce((a, b) => a + b, 0);
  $("redacted-summary").textContent =
    `${hidden} place(s) hidden. Highlighted placeholders such as [STUDENT_NAME] replace them. ${view.words.toLocaleString()} words remain.`;
  $("sections-found").textContent = view.sections_found.length
    ? `Sections recognised from your headings: ${view.sections_found.map((s) => s.replace(/_/g, " ")).join(", ")}. If a section is missing, the whole draft is sent for the checks that need it.`
    : "";
  $("redaction-result").hidden = false;
}

function toggleKeep(term, keepVisible) {
  const lower = term.toLowerCase();
  state.ids.excluded_terms = state.ids.excluded_terms.filter((t) => t.toLowerCase() !== lower);
  if (keepVisible) state.ids.excluded_terms.push(term);
  runRedaction();
}

$("btn-to-criteria").addEventListener("click", () => show("criteria"));
$("btn-restart-1").addEventListener("click", restart);

// ------------------------------------------------------------ 3. criteria

function buildCriteriaList() {
  const list = $("criteria-list");
  clear(list);
  for (const c of state.info.criteria) {
    list.append(el("label", {},
      el("input", { type: "checkbox", name: "criterion", value: c.id, checked: true }),
      el("span", {}, el("strong", {}, c.name), el("span", { class: "muted small" }, `${c.summary} (${c.checks} checks)`))));
  }
}

function selectedCriteria() {
  return [...document.querySelectorAll("input[name=criterion]:checked")].map((i) => i.value);
}

$("btn-back-redact").addEventListener("click", () => show("redact"));

$("btn-build").addEventListener("click", (e) => busy(e.currentTarget, async () => {
  const criteria = selectedCriteria();
  if (!criteria.length) {
    showBanner("Choose at least one criterion.");
    return;
  }
  const view = await call("build_payload", { criteria });
  if (!view) return;
  state.payload = view;
  renderPayload(view);
  show("payload");
}));

// ------------------------------------------------------------ 4. payload

function renderPayload(view) {
  const b = view.bundle;
  const summary = $("payload-summary");
  clear(summary);
  const row = (k, v) => summary.append(el("dt", {}, k), el("dd", {}, v));
  row("Sent to", b.engine.destination);
  row("Model", b.engine.primary_model);
  row("Second check model", b.engine.second_pass_model);
  row("Requests", `${b.requests.length} (one per check), plus one second check of the questions`);
  row("Total size", `${(view.total_bytes / 1024).toFixed(1)} KB`);
  row("Fingerprint (SHA-256)", b.sha256);
  if (!state.info.api_key_configured) {
    row("Status", "No API key is set up, so Send will fail without sending anything.");
  }
  $("data-handling").textContent = b.engine.data_handling;

  const list = $("request-list");
  clear(list);
  b.requests.forEach((r, i) => {
    const details = el("details", { class: "request" },
      el("summary", {}, `${i + 1}. ${r.label}`, el("span", { class: "scope" }, `${r.scope} · ${(r.body.length / 1024).toFixed(1)} KB`)));
    // Build the heavy content only when opened.
    details.addEventListener("toggle", () => {
      if (!details.open || details.dataset.filled) return;
      details.dataset.filled = "1";
      details.append(
        el("h3", {}, "Your redacted text in this request"),
        el("pre", { class: "excerpt" }, r.excerpt),
        el("h3", {}, `Exact bytes sent to ${r.url}`),
        el("pre", { class: "bytes" }, r.body),
      );
    });
    list.append(details);
  });
  $("second-pass-template").textContent = b.second_pass_template;
  $("chk-reviewed").checked = false;
  $("btn-send").disabled = true;
}

$("chk-reviewed").addEventListener("change", (e) => {
  $("btn-send").disabled = !e.currentTarget.checked;
});

$("btn-back-criteria").addEventListener("click", () => show("criteria"));

$("btn-send").addEventListener("click", async () => {
  if (!state.payload || !$("chk-reviewed").checked) return;
  const displayedHash = state.payload.bundle.sha256;
  $("progress").value = 0;
  $("progress").max = state.payload.bundle.requests.length || 1;
  $("progress-text").textContent = "Starting…";
  show("running");
  const result = await call("confirm_and_run", { displayedHash });
  state.payload = null; // a bundle can be sent only once
  if (!result) {
    show("criteria");
    showBanner($("banner").textContent + " Please review what will be sent again.");
    return;
  }
  renderResults(result);
  show("results");
});

listen("run-progress", (event) => {
  const { done, total } = event.payload;
  $("progress").max = total;
  $("progress").value = done;
  $("progress-text").textContent = `${done} of ${total} checks answered`;
});

// ------------------------------------------------------------ 5. results

function renderResults(result) {
  const r = result.receipt;
  $("results-summary").textContent =
    `${r.feedback_items_shown} question(s) shown. ${r.feedback_items_rejected} generated item(s) were blocked by the safety checks and not shown.`;

  const errors = $("results-errors");
  clear(errors);
  for (const e of result.errors) errors.append(el("p", { class: "warn" }, e));

  const list = $("results-list");
  clear(list);
  for (const cid of r.criteria_reviewed) {
    const criterion = state.info.criteria.find((c) => c.id === cid);
    list.append(el("h2", {}, criterion ? criterion.name : cid));
    const items = result.items.filter((it) => it.criterion === cid);
    if (!items.length) {
      list.append(el("p", { class: "muted" }, "No questions for this criterion in this run."));
    }
    for (const it of items) {
      list.append(el("div", { class: "item" },
        el("p", { class: "check" }, it.check_description),
        el("blockquote", {}, it.student_quote),
        el("p", { class: "q" }, it.question)));
    }
  }
  $("ack").textContent = r.acknowledgement_statement;
  $("save-status").textContent = "Nothing has been saved. Use the buttons above if you want to keep the questions or the receipt.";
}

$("btn-save-report").addEventListener("click", (e) => busy(e.currentTarget, async () => {
  const name = await call("save_report");
  if (name) $("save-status").textContent = `Saved ${name}.`;
}));

$("btn-save-receipt").addEventListener("click", (e) => busy(e.currentTarget, async () => {
  const name = await call("save_receipt");
  if (name) $("save-status").textContent = `Saved ${name}.`;
}));

$("btn-restart-2").addEventListener("click", restart);

async function restart() {
  await call("reset_session");
  state.ids = emptyIdentifiers();
  state.payload = null;
  $("form-ids").reset();
  clear($("redacted-text"));
  clear($("results-list"));
  show("open");
}

show("open");
init();
