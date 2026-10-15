//! Static audit of the source tree. These tests encode the trust guarantees as
//! rules a reviewer can read, and fail the build if code drifts from them.

mod common;

use std::path::{Path, PathBuf};

use common::repo_root;

fn files_under(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target" || n == "gen" || n == "node_modules") {
                continue;
            }
            out.extend(files_under(&p, exts));
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
    out
}

fn rust_sources() -> Vec<PathBuf> {
    let root = repo_root();
    let mut v = files_under(&root.join("core/src"), &["rs"]);
    v.extend(files_under(&root.join("src-tauri/src"), &["rs"]));
    v
}

fn rel(p: &Path) -> String {
    p.strip_prefix(repo_root()).unwrap().to_string_lossy().replace('\\', "/")
}

/// Strip `//` comments so that documentation mentioning a type is allowed.
fn code_only(text: &str) -> String {
    text.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

#[test]
fn only_net_rs_can_touch_the_network() {
    let forbidden = ["reqwest", "TcpStream", "UdpSocket", "TcpListener", "ureq", "hyper::", "std::net", "tauri_plugin_http", "tauri_plugin_updater"];
    let mut offenders = Vec::new();
    for f in rust_sources() {
        if rel(&f) == "core/src/net.rs" {
            continue;
        }
        let code = code_only(&std::fs::read_to_string(&f).unwrap());
        for word in forbidden {
            if code.contains(word) {
                offenders.push(format!("{} uses {word}", rel(&f)));
            }
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn core_never_writes_to_disk() {
    let writes = ["fs::write", "File::create", "OpenOptions", "create_dir", "fs::copy", "fs::rename", "set_permissions"];
    for f in files_under(&repo_root().join("core/src"), &["rs"]) {
        let code = code_only(&std::fs::read_to_string(&f).unwrap());
        for w in writes {
            assert!(!code.contains(w), "{} contains {w}: the core must not write files", rel(&f));
        }
    }
}

#[test]
fn desktop_shell_writes_only_in_the_two_save_commands() {
    let path = repo_root().join("src-tauri/src/commands.rs");
    let Ok(text) = std::fs::read_to_string(&path) else { return }; // shell not built yet
    let code = code_only(&text);
    let writes = code.matches("std::fs::write(").count();
    assert_eq!(writes, 3, "expected exactly 3 writes (report, receipt json, receipt html)");
    for (i, _) in code.match_indices("std::fs::write(") {
        let before = &code[..i];
        let fn_start = before.rfind("fn ").unwrap();
        let fn_name: String = before[fn_start + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        assert!(fn_name == "save_report" || fn_name == "save_receipt", "file write inside {fn_name}");
    }
    for other in ["File::create", "OpenOptions", "create_dir", "fs::copy"] {
        assert!(!code.contains(other));
    }
    let main = code_only(&std::fs::read_to_string(repo_root().join("src-tauri/src/main.rs")).unwrap());
    for w in ["fs::write", "File::create", "OpenOptions"] {
        assert!(!main.contains(w), "main.rs contains {w}");
    }
}

#[test]
fn confirmation_is_only_created_by_the_send_command() {
    for f in rust_sources() {
        let code = code_only(&std::fs::read_to_string(&f).unwrap());
        let r = rel(&f);
        if code.contains("payload::confirm(") || code.contains("confirm(bundle") {
            assert!(r == "src-tauri/src/commands.rs" || r == "core/src/payload.rs", "{r} creates a ConfirmedPayload");
        }
        if r == "src-tauri/src/commands.rs" {
            assert_eq!(code.matches("payload::confirm(").count(), 1, "exactly one confirmation site");
        }
    }
}

#[test]
fn ui_has_no_network_and_no_html_injection() {
    let ui = files_under(&repo_root().join("ui"), &["js", "html", "css"]);
    if ui.is_empty() {
        return;
    }
    let forbidden = ["fetch(", "XMLHttpRequest", "WebSocket", "EventSource", "sendBeacon", "http://", "https://",
        "innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function", "localStorage",
        "sessionStorage", "indexedDB", "<textarea"];
    for f in ui {
        let text = std::fs::read_to_string(&f).unwrap();
        for word in forbidden {
            assert!(!text.contains(word), "{} contains {word}", rel(&f));
        }
    }
}

#[test]
fn tauri_config_is_locked_down() {
    let path = repo_root().join("src-tauri/tauri.conf.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let conf: serde_json::Value = serde_json::from_str(&text).unwrap();
    let csp = conf["app"]["security"]["csp"].as_str().expect("CSP set");
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("connect-src ipc: http://ipc.localhost"), "{csp}");
    assert!(!csp.contains("https:"), "CSP must not allow external hosts: {csp}");
    assert!(conf["plugins"].as_object().map_or(true, |p| p.keys().all(|k| k == "dialog")), "only the dialog plugin");
    assert_eq!(conf["app"]["windows"].as_array().map(|a| a.len()).unwrap_or(0), 0, "window is created in code (incognito)");

    let cap: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo_root().join("src-tauri/capabilities/default.json")).unwrap()).unwrap();
    let perms: Vec<&str> = cap["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    assert_eq!(perms, vec!["core:event:allow-listen", "core:event:allow-unlisten"], "UI gets no file, shell, http or dialog permissions");
}

#[test]
fn no_real_student_work_in_samples() {
    // Every sample must declare itself synthetic in the generator, and use
    // reserved example domains / fictional phone ranges.
    let gen = std::fs::read_to_string(repo_root().join("tools/make_samples.py")).unwrap();
    assert!(gen.contains("SYNTHETIC"));
    for f in files_under(&repo_root().join("samples/source"), &["md"]) {
        let t = std::fs::read_to_string(&f).unwrap();
        for email in t.split_whitespace().filter(|w| w.contains('@')) {
            assert!(email.contains("example."), "{}: non-example e-mail {email}", rel(&f));
        }
    }
}
