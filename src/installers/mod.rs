pub mod paths;
pub mod io;
pub mod marker_block;
pub mod json_hook;
pub mod common;
pub mod claude_code;
pub mod gemini;
pub mod tabnine;
pub mod cursor;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Scope {
    Local(PathBuf),
    Global,
}

#[derive(Debug, Clone)]
pub struct InstallOpts {
    pub min_lines: usize,
    pub always: bool,
    pub dry_run: bool,
    pub force: bool,
}

impl Default for InstallOpts {
    fn default() -> Self {
        Self {
            min_lines: 200,
            always: false,
            dry_run: false,
            force: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Change {
    Created(PathBuf),
    Updated(PathBuf),
    Removed(PathBuf),
    Skipped { path: PathBuf, reason: String },
    NotApplicable,
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub present: bool,
    pub config_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub prompt_installed: bool,
    pub prompt_version: Option<String>,
    pub hook_installed: bool,
}

pub trait Installer: Sync + Send {
    fn name(&self) -> &'static str;
    fn detect(&self, scope: &Scope) -> Detection;
    fn install_prompt(&self, scope: &Scope, opts: &InstallOpts) -> Result<Change, String>;
    fn install_hook(&self, scope: &Scope, opts: &InstallOpts) -> Result<Change, String>;
    fn uninstall(&self, scope: &Scope, opts: &InstallOpts) -> Result<Vec<Change>, String>;
    fn status(&self, scope: &Scope) -> Status;
}

pub fn registry() -> Vec<Box<dyn Installer>> {
    vec![
        Box::new(claude_code::ClaudeCode),
        Box::new(gemini::Gemini),
        Box::new(tabnine::Tabnine),
        Box::new(cursor::Cursor),
    ]
}
