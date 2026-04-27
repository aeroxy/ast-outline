use std::path::PathBuf;

use super::paths;
use super::{common, Change, Detection, InstallOpts, Installer, Scope, Status};
use crate::prompt::AGENT_PROMPT;

pub struct Cursor;

impl Cursor {
    fn prompt_path(&self, scope: &Scope) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join(".cursor/rules/ast-outline.mdc")),
            Scope::Global => paths::under_home(".cursor/User Rules.md"),
        }
    }
}

impl Installer for Cursor {
    fn name(&self) -> &'static str {
        "cursor"
    }

    fn detect(&self, scope: &Scope) -> Detection {
        let p = match self.prompt_path(scope) {
            Ok(p) => p,
            Err(_) => {
                return Detection {
                    present: false,
                    config_root: None,
                }
            }
        };
        let root = p.parent().map(|p| p.to_path_buf());
        Detection {
            present: root.as_ref().map(|r| r.exists()).unwrap_or(false),
            config_root: root,
        }
    }

    fn install_prompt(&self, scope: &Scope, opts: &InstallOpts) -> Result<Change, String> {
        common::install_prompt_in(&self.prompt_path(scope)?, AGENT_PROMPT, opts)
    }

    fn install_hook(&self, _scope: &Scope, _opts: &InstallOpts) -> Result<Change, String> {
        Ok(Change::NotApplicable)
    }

    fn uninstall(&self, scope: &Scope, opts: &InstallOpts) -> Result<Vec<Change>, String> {
        let mut changes = Vec::new();
        if let Some(c) = common::uninstall_prompt_in(&self.prompt_path(scope)?, opts)? {
            changes.push(c);
        }
        Ok(changes)
    }

    fn status(&self, scope: &Scope) -> Status {
        common::status_for_prompt_only(self.prompt_path(scope).ok().as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_writes_to_cursor_rules() {
        let dir = TempDir::new().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        Cursor
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        let p = dir.path().join(".cursor/rules/ast-outline.mdc");
        assert!(p.exists());
        let contents = std::fs::read_to_string(&p).unwrap();
        assert!(contents.contains("ast-outline:begin"));
    }

    #[test]
    fn install_hook_is_not_applicable() {
        let dir = TempDir::new().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        let change = Cursor
            .install_hook(&scope, &InstallOpts::default())
            .unwrap();
        assert!(matches!(change, Change::NotApplicable));
    }
}
