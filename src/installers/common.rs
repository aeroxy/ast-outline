use std::path::Path;

use serde_json::Value;

use super::io::{atomic_write, read_optional};
use super::json_hook;
use super::marker_block::{self, ApplyOutcome};
use super::{Change, InstallOpts, Status};

pub fn install_prompt_in(
    path: &Path,
    snippet: &str,
    opts: &InstallOpts,
) -> Result<Change, String> {
    let existing = read_optional(path)?.unwrap_or_default();
    let body = snippet.trim_end_matches('\n').to_string() + "\n";
    let (new_contents, outcome) = marker_block::apply(
        &existing,
        &body,
        &body,
        snippet,
        env!("CARGO_PKG_VERSION"),
        opts.force,
    );
    match outcome {
        ApplyOutcome::UserEditsBlocked(diff) => Err(format!(
            "{}: user edits inside marker block; pass --force to overwrite\n{}",
            path.display(),
            diff
        )),
        _ => {
            if existing == new_contents {
                return Ok(Change::Skipped {
                    path: path.to_path_buf(),
                    reason: "already up to date".into(),
                });
            }
            if !opts.dry_run {
                atomic_write(path, &new_contents)?;
            }
            Ok(if existing.is_empty() {
                Change::Created(path.to_path_buf())
            } else {
                Change::Updated(path.to_path_buf())
            })
        }
    }
}

pub fn install_json_hook_in<F>(
    path: &Path,
    hook_path: &[&str],
    entry: Value,
    matches: F,
    opts: &InstallOpts,
) -> Result<Change, String>
where
    F: Fn(&Value) -> bool,
{
    let existing = read_optional(path)?.unwrap_or_else(|| "{}".into());
    let mut root: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    let modified = json_hook::upsert(&mut root, hook_path, entry, matches);
    if !modified {
        return Ok(Change::Skipped {
            path: path.to_path_buf(),
            reason: "already up to date".into(),
        });
    }
    let new_contents = serde_json::to_string_pretty(&root).unwrap() + "\n";
    if !opts.dry_run {
        atomic_write(path, &new_contents)?;
    }
    Ok(if existing.trim() == "{}" || existing.is_empty() {
        Change::Created(path.to_path_buf())
    } else {
        Change::Updated(path.to_path_buf())
    })
}

pub fn uninstall_prompt_in(path: &Path, opts: &InstallOpts) -> Result<Option<Change>, String> {
    let Some(existing) = read_optional(path)? else {
        return Ok(None);
    };
    let (out, removed) = marker_block::remove(&existing);
    if !removed {
        return Ok(None);
    }
    if !opts.dry_run {
        atomic_write(path, &out)?;
    }
    Ok(Some(Change::Removed(path.to_path_buf())))
}

pub fn uninstall_json_hook_in<F>(
    path: &Path,
    hook_path: &[&str],
    matches: F,
    opts: &InstallOpts,
) -> Result<Option<Change>, String>
where
    F: Fn(&Value) -> bool,
{
    let Some(existing) = read_optional(path)? else {
        return Ok(None);
    };
    let mut root: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    if !json_hook::remove(&mut root, hook_path, matches) {
        return Ok(None);
    }
    let new_contents = serde_json::to_string_pretty(&root).unwrap() + "\n";
    if !opts.dry_run {
        atomic_write(path, &new_contents)?;
    }
    Ok(Some(Change::Removed(path.to_path_buf())))
}

pub fn status_for<F>(
    prompt_path: Option<&Path>,
    settings_path: Option<&Path>,
    hook_path: &[&str],
    matches: F,
) -> Status
where
    F: Fn(&Value) -> bool,
{
    let mut s = Status {
        prompt_installed: false,
        prompt_version: None,
        hook_installed: false,
    };
    if let Some(pp) = prompt_path {
        if let Ok(Some(contents)) = read_optional(pp) {
            s.prompt_version = marker_block::installed_version(&contents);
            s.prompt_installed = s.prompt_version.is_some();
        }
    }
    if let Some(sp) = settings_path {
        if let Ok(Some(contents)) = read_optional(sp) {
            if let Ok(root) = serde_json::from_str::<Value>(&contents) {
                s.hook_installed = json_hook::is_installed(&root, hook_path, matches);
            }
        }
    }
    s
}

pub fn status_for_prompt_only(prompt_path: Option<&Path>) -> Status {
    let mut s = Status {
        prompt_installed: false,
        prompt_version: None,
        hook_installed: false,
    };
    if let Some(pp) = prompt_path {
        if let Ok(Some(contents)) = read_optional(pp) {
            s.prompt_version = marker_block::installed_version(&contents);
            s.prompt_installed = s.prompt_version.is_some();
        }
    }
    s
}
