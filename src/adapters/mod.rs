use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;

pub mod claude_code;
pub mod opencode;
pub mod gemini;

pub trait Adapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>>;
}

#[allow(dead_code)]
pub fn conformance_report() {}
