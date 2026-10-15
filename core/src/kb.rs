//! Knowledge base loader. The KB is a folder of human-editable YAML files:
//!
//!   manifest.yaml            kb_id, kb_version, syllabus_year, list of criterion files
//!   <criterion>.yaml         checks for one criterion
//!   whole-document.yaml      checks that need the full text
//!
//! Every file repeats kb_version and syllabus_year; the loader refuses a KB
//! whose files disagree, so a half-updated KB cannot be used by accident.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sections::SECTION_IDS;

pub const STATUS_DRAFT: &str = "draft_needs_teacher_review";
pub const STATUS_APPROVED: &str = "teacher_approved";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub kb_id: String,
    pub subject: String,
    pub assessment: String,
    pub kb_version: String,
    pub syllabus_year: u32,
    pub status: String,
    pub notice: String,
    pub criteria: Vec<CriterionRef>,
    pub whole_document_file: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionRef {
    pub id: String,
    pub name: String,
    pub file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckFile {
    kb_version: String,
    syllabus_year: u32,
    #[serde(default)]
    criterion: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Sections,
    WholeDocument,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub criterion: String,
    pub status: String,
    #[serde(default)]
    pub scope: Scope,
    pub description: String,
    pub what_to_look_for: String,
    pub common_weakness_patterns: Vec<String>,
    pub question_templates: Vec<String>,
    pub applies_to_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CriterionDef {
    pub id: String,
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct KnowledgeBase {
    pub manifest: Manifest,
    pub criteria: Vec<CriterionDef>,
    /// All checks, criteria files first (in manifest order), then whole-document checks.
    pub checks: Vec<Check>,
    /// SHA-256 over every KB file, in load order (recorded in the receipt).
    pub sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum KbError {
    #[error("cannot read knowledge-base file {0}: {1}")]
    Io(String, String),
    #[error("invalid knowledge-base file {0}: {1}")]
    Yaml(String, String),
    #[error("knowledge base is inconsistent: {0}")]
    Invalid(String),
}

fn read(dir: &Path, file: &str, hasher: &mut Sha256) -> Result<String, KbError> {
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return Err(KbError::Invalid(format!("file name {file:?} must be a plain file name")));
    }
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| KbError::Io(file.into(), e.to_string()))?;
    hasher.update(file.as_bytes());
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    Ok(text)
}

pub fn load(dir: &Path) -> Result<KnowledgeBase, KbError> {
    let mut hasher = Sha256::new();
    let manifest_text = read(dir, "manifest.yaml", &mut hasher)?;
    let manifest: Manifest = serde_yaml::from_str(&manifest_text)
        .map_err(|e| KbError::Yaml("manifest.yaml".into(), e.to_string()))?;

    let mut criteria = Vec::new();
    let mut checks = Vec::new();

    let mut files: Vec<(String, Option<String>)> = manifest
        .criteria
        .iter()
        .map(|c| (c.file.clone(), Some(c.id.clone())))
        .collect();
    files.push((manifest.whole_document_file.clone(), None));

    for (file, expected_criterion) in &files {
        let text = read(dir, file, &mut hasher)?;
        let cf: CheckFile =
            serde_yaml::from_str(&text).map_err(|e| KbError::Yaml(file.clone(), e.to_string()))?;
        if cf.kb_version != manifest.kb_version || cf.syllabus_year != manifest.syllabus_year {
            return Err(KbError::Invalid(format!(
                "{file} has kb_version {} / syllabus_year {}, manifest has {} / {}",
                cf.kb_version, cf.syllabus_year, manifest.kb_version, manifest.syllabus_year
            )));
        }
        match expected_criterion {
            Some(cid) => {
                if cf.criterion.as_deref() != Some(cid.as_str()) {
                    return Err(KbError::Invalid(format!("{file} must declare criterion: {cid}")));
                }
                let name = manifest.criteria.iter().find(|c| &c.id == cid).unwrap().name.clone();
                criteria.push(CriterionDef {
                    id: cid.clone(),
                    name,
                    summary: cf.summary.clone().unwrap_or_default(),
                });
                for c in &cf.checks {
                    if &c.criterion != cid {
                        return Err(KbError::Invalid(format!("check {} in {file} must have criterion {cid}", c.id)));
                    }
                    if c.scope != Scope::Sections {
                        return Err(KbError::Invalid(format!(
                            "check {} : whole-document checks belong in {}",
                            c.id, manifest.whole_document_file
                        )));
                    }
                }
            }
            None => {
                for c in &cf.checks {
                    if c.scope != Scope::WholeDocument {
                        return Err(KbError::Invalid(format!("check {} in {file} must have scope: whole_document", c.id)));
                    }
                }
            }
        }
        checks.extend(cf.checks);
    }

    validate(&manifest, &checks)?;
    Ok(KnowledgeBase { manifest, criteria, checks, sha256: hex::encode(hasher.finalize()) })
}

fn validate(manifest: &Manifest, checks: &[Check]) -> Result<(), KbError> {
    let criterion_ids: HashSet<&str> = manifest.criteria.iter().map(|c| c.id.as_str()).collect();
    let mut seen = HashSet::new();
    for c in checks {
        let bad = |msg: &str| KbError::Invalid(format!("check {}: {msg}", c.id));
        if !seen.insert(c.id.as_str()) {
            return Err(bad("duplicate id"));
        }
        if !criterion_ids.contains(c.criterion.as_str()) {
            return Err(bad("unknown criterion"));
        }
        if c.status != STATUS_DRAFT && c.status != STATUS_APPROVED {
            return Err(bad("status must be draft_needs_teacher_review or teacher_approved"));
        }
        if c.question_templates.is_empty() || c.common_weakness_patterns.is_empty() {
            return Err(bad("needs at least one weakness pattern and one question template"));
        }
        if let Some(t) = c.question_templates.iter().find(|t| !t.trim_end().ends_with('?')) {
            return Err(bad(&format!("question template must end with '?': {t}")));
        }
        if c.scope == Scope::Sections && c.applies_to_sections.is_empty() {
            return Err(bad("applies_to_sections must not be empty"));
        }
        if let Some(s) = c.applies_to_sections.iter().find(|s| !SECTION_IDS.contains(&s.as_str())) {
            return Err(bad(&format!("unknown section id {s}")));
        }
    }
    Ok(())
}

impl KnowledgeBase {
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }

    pub fn criterion_name(&self, id: &str) -> String {
        self.criteria.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Checks for the selected criteria, in KB order.
    pub fn checks_for(&self, criteria: &[String]) -> Vec<&Check> {
        self.checks.iter().filter(|c| criteria.contains(&c.criterion)).collect()
    }

    pub fn has_unreviewed_checks(&self) -> bool {
        self.checks.iter().any(|c| c.status != STATUS_APPROVED)
    }

    /// Position used to sort feedback in a stable, KB-defined order.
    pub fn sort_key(&self, criterion: &str, check: &str) -> (usize, usize) {
        let ci = self.criteria.iter().position(|c| c.id == criterion).unwrap_or(usize::MAX);
        let ki = self.checks.iter().position(|c| c.id == check).unwrap_or(usize::MAX);
        (ci, ki)
    }
}
