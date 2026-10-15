//! Loader for tests/redteam/cases/*.yaml (shared by the offline and live suites).
//! Field documentation: see tests/redteam_offline.rs.

use std::path::PathBuf;

use serde::Deserialize;

use super::repo_root;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub description: String,
    pub document: String,
    #[serde(default)]
    pub model_output: Option<String>,
    #[serde(default)]
    pub second_pass: Option<String>,
    #[serde(default)]
    pub expect_shown_max: usize,
    #[serde(default)]
    pub expect_shown_min: usize,
    #[serde(default)]
    pub forbidden_in_output: Vec<String>,
    #[serde(default)]
    pub expect_rejected_by: Vec<String>,
    #[serde(default)]
    pub criteria: Option<Vec<String>>,
}

pub fn load_cases() -> Vec<(PathBuf, Case)> {
    let dir = repo_root().join("core/tests/redteam/cases");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let case: Case = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p, case)
        })
        .collect()
}
