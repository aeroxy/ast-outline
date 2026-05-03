use std::path::PathBuf;

use serde_json::{json, Value};

use super::json_hook::MARKER;
use super::paths;
use super::{common, Change, Detection, InstallOpts, Installer, Scope, Status};
use crate::prompt::AGENT_PROMPT;

pub struct ClaudeCode;

const HOOK_PATH: &[&str] = &["hooks", "PreToolUse"];

/// Built-in Claude Code subagents that run in their own context and never see
/// `CLAUDE.md`. Shadowing them with `.claude/agents/<Name>.md` is the official
/// way to push the ast-outline prompt into their system prompt.
const SHADOWED_SUBAGENTS: &[&str] = &["Explore"];

impl ClaudeCode {
    fn prompt_path(&self, scope: &Scope) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join("CLAUDE.md")),
            Scope::Global => paths::under_home(".claude/CLAUDE.md"),
        }
    }
    fn settings_path(&self, scope: &Scope) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join(".claude/settings.json")),
            Scope::Global => paths::under_home(".claude/settings.json"),
        }
    }
    fn subagent_path(&self, scope: &Scope, name: &str) -> Result<PathBuf, String> {
        match scope {
            Scope::Local(root) => Ok(root.join(".claude/agents").join(format!("{}.md", name))),
            Scope::Global => paths::under_home(&format!(".claude/agents/{}.md", name)),
        }
    }
    fn hook_command(&self, opts: &InstallOpts) -> String {
        let mut cmd = format!(
            "ast-outline hook --protocol claude-code --min-lines {}",
            opts.min_lines
        );
        if opts.always {
            cmd.push_str(" --always");
        }
        cmd
    }
    fn hook_entry(&self, opts: &InstallOpts) -> Value {
        json!({
            "matcher": "Read",
            "hooks": [{ "type": "command", "command": self.hook_command(opts) }]
        })
    }
}

fn matches_entry(v: &Value) -> bool {
    v.get("matcher").and_then(|m| m.as_str()) == Some("Read")
        && v.get("hooks")
            .and_then(|h| h.as_array())
            .and_then(|h| h.first())
            .and_then(|h0| h0.get("command"))
            .and_then(|c| c.as_str())
            .map(|c| c.starts_with(MARKER))
            .unwrap_or(false)
}

impl Installer for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn detect(&self, scope: &Scope) -> Detection {
        let dir_exists = self
            .prompt_path(scope)
            .ok()
            .and_then(|p| p.parent().map(|r| r.to_path_buf()))
            .map(|r| r.exists())
            .unwrap_or(false);
        Detection {
            present: dir_exists || paths::binary_on_path("claude"),
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

    fn install_subagents(&self, scope: &Scope, opts: &InstallOpts) -> Result<Vec<Change>, String> {
        let mut changes = Vec::with_capacity(SHADOWED_SUBAGENTS.len());
        for name in SHADOWED_SUBAGENTS {
            let path = self.subagent_path(scope, name)?;
            changes.push(common::install_prompt_in(&path, AGENT_PROMPT, opts)?);
        }
        Ok(changes)
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
        for name in SHADOWED_SUBAGENTS {
            if let Some(c) = common::uninstall_prompt_in(&self.subagent_path(scope, name)?, opts)? {
                changes.push(c);
            }
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

    fn local_scope(dir: &TempDir) -> Scope {
        Scope::Local(dir.path().to_path_buf())
    }

    #[test]
    fn install_prompt_creates_file_with_marker_block() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let change = ClaudeCode
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        assert!(matches!(change, Change::Created(_)));
        let contents = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(contents.contains("<!-- ast-outline:begin"));
        assert!(contents.contains("ast-outline"));
    }

    #[test]
    fn install_prompt_idempotent() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        ClaudeCode
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        let after_first = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        ClaudeCode
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        let after_second = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn install_hook_creates_settings_with_entry() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let change = ClaudeCode
            .install_hook(&scope, &InstallOpts::default())
            .unwrap();
        assert!(matches!(change, Change::Created(_)));
        let contents = std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        assert!(contents.contains("--protocol claude-code"));
        assert!(contents.contains("\"matcher\": \"Read\""));
    }

    #[test]
    fn install_hook_preserves_other_hooks() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"echo hi"}]}]}}"#,
        ).unwrap();
        let scope = local_scope(&dir);
        ClaudeCode
            .install_hook(&scope, &InstallOpts::default())
            .unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        assert!(contents.contains("echo hi"));
        assert!(contents.contains("--protocol claude-code"));
    }

    #[test]
    fn uninstall_removes_block_and_hook_keeps_siblings() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"echo hi"}]}]}}"#,
        ).unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts::default();
        ClaudeCode.install_prompt(&scope, &opts).unwrap();
        ClaudeCode.install_hook(&scope, &opts).unwrap();
        let removed = ClaudeCode.uninstall(&scope, &opts).unwrap();
        assert_eq!(removed.len(), 2);
        let prompt = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(!prompt.contains("ast-outline:begin"));
        let settings = std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        assert!(settings.contains("echo hi"));
        assert!(!settings.contains("ast-outline hook"));
    }

    #[test]
    fn status_reports_versions_and_flags() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let s0 = ClaudeCode.status(&scope);
        assert!(!s0.prompt_installed);
        assert!(!s0.hook_installed);
        ClaudeCode
            .install_prompt(&scope, &InstallOpts::default())
            .unwrap();
        ClaudeCode
            .install_hook(&scope, &InstallOpts::default())
            .unwrap();
        let s1 = ClaudeCode.status(&scope);
        assert!(s1.prompt_installed);
        assert!(s1.hook_installed);
        assert_eq!(s1.prompt_version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn dry_run_does_not_write() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts { dry_run: true, ..Default::default() };
        ClaudeCode.install_prompt(&scope, &opts).unwrap();
        assert!(!dir.path().join("CLAUDE.md").exists());
    }

    #[test]
    fn install_subagents_creates_explore_md_with_marker_block() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let changes = ClaudeCode
            .install_subagents(&scope, &InstallOpts::default())
            .unwrap();
        assert_eq!(changes.len(), SHADOWED_SUBAGENTS.len());
        assert!(matches!(changes[0], Change::Created(_)));
        let path = dir.path().join(".claude/agents/Explore.md");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("<!-- ast-outline:begin"));
        assert!(contents.contains("ast-outline"));
    }

    #[test]
    fn install_subagents_idempotent() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts::default();
        ClaudeCode.install_subagents(&scope, &opts).unwrap();
        let path = dir.path().join(".claude/agents/Explore.md");
        let after_first = std::fs::read_to_string(&path).unwrap();
        let changes = ClaudeCode.install_subagents(&scope, &opts).unwrap();
        assert!(matches!(changes[0], Change::Skipped { .. }));
        let after_second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn install_subagents_wraps_legacy_explore_md_in_place() {
        // Simulates a user who manually created ~/.claude/agents/Explore.md by
        // pasting `ast-outline prompt` output before this installer existed.
        let dir = TempDir::new().unwrap();
        let agent_path = dir.path().join(".claude/agents/Explore.md");
        std::fs::create_dir_all(agent_path.parent().unwrap()).unwrap();
        std::fs::write(&agent_path, AGENT_PROMPT).unwrap();
        let scope = local_scope(&dir);
        let changes = ClaudeCode
            .install_subagents(&scope, &InstallOpts::default())
            .unwrap();
        assert!(matches!(changes[0], Change::Updated(_)));
        let contents = std::fs::read_to_string(&agent_path).unwrap();
        assert!(contents.contains("<!-- ast-outline:begin"));
        // Body is wrapped exactly once — the legacy bare snippet is gone.
        assert_eq!(contents.matches("## Prefer `ast-outline` over full reads").count(), 1);
    }

    #[test]
    fn install_subagents_appends_to_user_customized_file() {
        let dir = TempDir::new().unwrap();
        let agent_path = dir.path().join(".claude/agents/Explore.md");
        std::fs::create_dir_all(agent_path.parent().unwrap()).unwrap();
        let custom = "---\nname: Explore\ntools: Read, Grep\n---\nUser prompt body.\n";
        std::fs::write(&agent_path, custom).unwrap();
        let scope = local_scope(&dir);
        ClaudeCode
            .install_subagents(&scope, &InstallOpts::default())
            .unwrap();
        let contents = std::fs::read_to_string(&agent_path).unwrap();
        assert!(contents.starts_with(custom));
        assert!(contents.contains("<!-- ast-outline:begin"));
    }

    #[test]
    fn uninstall_removes_subagent_block_and_keeps_user_content() {
        let dir = TempDir::new().unwrap();
        let agent_path = dir.path().join(".claude/agents/Explore.md");
        std::fs::create_dir_all(agent_path.parent().unwrap()).unwrap();
        let custom = "---\nname: Explore\n---\nKeep me.\n";
        std::fs::write(&agent_path, custom).unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts::default();
        ClaudeCode.install_subagents(&scope, &opts).unwrap();
        let removed = ClaudeCode.uninstall(&scope, &opts).unwrap();
        assert!(removed.iter().any(|c| matches!(c, Change::Removed(p) if p.ends_with("Explore.md"))));
        let contents = std::fs::read_to_string(&agent_path).unwrap();
        assert!(!contents.contains("ast-outline:begin"));
        assert!(contents.contains("Keep me."));
    }

    #[test]
    fn uninstall_subagent_noop_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts::default();
        let removed = ClaudeCode.uninstall(&scope, &opts).unwrap();
        assert!(removed.iter().all(|c| !matches!(c, Change::Removed(p) if p.ends_with("Explore.md"))));
    }

    #[test]
    fn install_subagents_dry_run_does_not_write() {
        let dir = TempDir::new().unwrap();
        let scope = local_scope(&dir);
        let opts = InstallOpts { dry_run: true, ..Default::default() };
        ClaudeCode.install_subagents(&scope, &opts).unwrap();
        assert!(!dir.path().join(".claude/agents/Explore.md").exists());
    }
}
