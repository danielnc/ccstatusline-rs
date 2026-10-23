//! The editor half of the transcript widgets (upstream `Tokens*.ts`, `Context*.ts`, `*Speed.ts`, `Session*.ts`,
//! `Cache*.ts`, ...): shared editor helpers for this group live here; each widget's own methods sit on its impl.

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &["context-length"];
