//! Skill invocations recorded by the `--hook` handler (port of upstream
//! `src/utils/skills.ts` `getSkillsMetrics`). The log is one small line per
//! invocation, so it is read whole like upstream does.

use super::json::{parse_value, JVal};
use crate::jsnum::js_trim;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillsMetrics {
    pub total_invocations: usize,
    pub unique_skills: Vec<String>,
    pub last_skill: Option<String>,
}

/// `getSkillsMetrics(sessionId)`
pub fn skills_metrics(session_id: &str) -> SkillsMetrics {
    let Some(path) = crate::hooks::skills_file_path(session_id) else { return SkillsMetrics::default() };
    let Ok(bytes) = std::fs::read(&path) else { return SkillsMetrics::default() };
    let text = String::from_utf8_lossy(&bytes);
    let mut skills: Vec<String> = Vec::new();
    for line in js_trim(&text).split('\n') {
        if js_trim(line).is_empty() {
            continue;
        }
        let Some(v @ JVal::Obj(_)) = parse_value(line.as_bytes()) else { continue };
        let (Some(JVal::Str(skill)), Some(JVal::Str(_))) = (v.get("skill"), v.get("session_id")) else { continue };
        skills.push(skill.clone());
    }
    if skills.is_empty() {
        return SkillsMetrics::default();
    }
    let mut unique: Vec<String> = Vec::new();
    for s in skills.iter().rev() {
        if !s.is_empty() && !unique.contains(s) {
            unique.push(s.clone());
        }
    }
    SkillsMetrics { total_invocations: skills.len(), unique_skills: unique, last_skill: skills.last().cloned() }
}
