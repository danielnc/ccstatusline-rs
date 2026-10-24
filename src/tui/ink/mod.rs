//! A port of the slice of Ink (and the libraries it bundles) that upstream's editor renders through, so each
//! screen reaches the terminal as the same characters in the same cells.

pub mod chalk;
pub mod gradient;
pub mod keys;
pub mod layout;
#[cfg(test)]
mod layout_oracle_tests;
pub mod output;
pub mod term;
pub mod text;
