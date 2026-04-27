use std::path::PathBuf;

use serde_json::{json, Value};

use super::json_hook::MARKER;
use super::paths;
use super::{common, Change, Detection, InstallOpts, Installer, Scope, Status};
use crate::prompt::AGENT_PROMPT;

pub struct Gemini;

const HOOK_PATH: &[&str] = &["hooks", "BeforeTool"];
const HOOK_NAME: &str = "ast-outline-read-interceptor";

impl Gemini {
    pub(crate) fn prompt_path(&self, scope: &Scope) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join("GEMINI.md")),
            Scope::Global => paths::under_home(".gemini/GEMINI.md"),
        }
    }
    pub(crate) fn settings_path(&self, scope: &Scope) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join(".gemini/settings.json")),
            Scope::Global => paths::under_home(".gemini/settings.json"),
        }
    }
    pub(crate) fn hook_entry(&self, opts: &InstallOpts) -> Value {
        let mut cmd = format!(
            "ast-outline hook --protocol gemini --min-lines {}",
            opts.min_lines
        );
        if opts.always {
            cmd.push_str(" --always");
        }
        json!({
            "matcher": "read_file",
            "hooks": [{ "name": HOOK_NAME, "type": "command", "command": cmd }]
        })
    }
}

pub(crate) fn matches_entry(v: &Value) -> bool {
    v.get("matcher").and_then(|m| m.as_str()) == Some("read_file")
        && v.get("hooks")
            .and_then(|h| h.as_array())
            .and_then(|h| h.first())
            .map(|h0| {
                h0.get("name").and_then(|n| n.as_str()) == Some(HOOK_NAME)
                    || h0
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.starts_with(MARKER))
                        .unwrap_or(false)
            })
            .unwrap_or(false)
}

impl Installer for Gemini {
    fn name(&self) -> &'static str {
        "gemini"
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

    fn install_hook(&self, scope: &Scope, opts: &InstallOpts) -> Result<Change, String> {
        common::install_json_hook_in(
            &self.settings_path(scope)?,
            HOOK_PATH,
            self.hook_entry(opts),
            matches_entry,
            opts,
        )
    }

    fn uninstall(&self, scope: &Scope, opts: &InstallOpts) -> Result<Vec<Change>, String> {
        let mut changes = Vec::new();
        if let Some(c) = common::uninstall_prompt_in(&self.prompt_path(scope)?, opts)? {
            changes.push(c);
        }
        if let Some(c) =
            common::uninstall_json_hook_in(&self.settings_path(scope)?, HOOK_PATH, matches_entry, opts)?
        {
            changes.push(c);
        }
        Ok(changes)
    }

    fn status(&self, scope: &Scope) -> Status {
        common::status_for(
            self.prompt_path(scope).ok().as_deref(),
            self.settings_path(scope).ok().as_deref(),
            HOOK_PATH,
            matches_entry,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_creates_gemini_md_and_settings() {
        let dir = TempDir::new().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        Gemini
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        Gemini
            .install_hook(&scope, &InstallOpts::default())
            .unwrap();
        let prompt = std::fs::read_to_string(dir.path().join("GEMINI.md")).unwrap();
        let settings = std::fs::read_to_string(dir.path().join(".gemini/settings.json")).unwrap();
        assert!(prompt.contains("ast-outline:begin"));
        assert!(settings.contains("--protocol gemini"));
        assert!(settings.contains("\"matcher\": \"read_file\""));
        assert!(settings.contains("BeforeTool"));
    }
}
