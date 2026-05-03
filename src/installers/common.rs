use std::path::Path;

use serde_json::Value;

use super::io::{atomic_write, read_optional};
use super::json_hook;
use super::json_object;
use super::marker_block::{self, ApplyOutcome};
use super::toml_object;
use super::{Change, InstallOpts, Status};

use toml_edit::{DocumentMut, Table};

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

pub fn install_json_object_in(
    path: &Path,
    key_path: &[&str],
    key: &str,
    entry: Value,
    opts: &InstallOpts,
) -> Result<Change, String> {
    let existing = read_optional(path)?.unwrap_or_else(|| "{}".into());
    let mut root: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    let modified = json_object::upsert(&mut root, key_path, key, entry);
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

pub fn uninstall_json_object_in(
    path: &Path,
    key_path: &[&str],
    key: &str,
    opts: &InstallOpts,
) -> Result<Option<Change>, String> {
    let Some(existing) = read_optional(path)? else {
        return Ok(None);
    };
    let mut root: Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    if !json_object::remove(&mut root, key_path, key) {
        return Ok(None);
    }
    let new_contents = serde_json::to_string_pretty(&root).unwrap() + "\n";
    if !opts.dry_run {
        atomic_write(path, &new_contents)?;
    }
    Ok(Some(Change::Removed(path.to_path_buf())))
}

pub fn install_toml_object_in(
    path: &Path,
    parent: &str,
    key: &str,
    entry: Table,
    opts: &InstallOpts,
) -> Result<Change, String> {
    let existing = read_optional(path)?.unwrap_or_default();
    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    let modified = toml_object::upsert(&mut doc, parent, key, entry);
    if !modified {
        return Ok(Change::Skipped {
            path: path.to_path_buf(),
            reason: "already up to date".into(),
        });
    }
    let new_contents = doc.to_string();
    if !opts.dry_run {
        atomic_write(path, &new_contents)?;
    }
    Ok(if existing.is_empty() {
        Change::Created(path.to_path_buf())
    } else {
        Change::Updated(path.to_path_buf())
    })
}

pub fn uninstall_toml_object_in(
    path: &Path,
    parent: &str,
    key: &str,
    opts: &InstallOpts,
) -> Result<Option<Change>, String> {
    let Some(existing) = read_optional(path)? else {
        return Ok(None);
    };
    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    if !toml_object::remove(&mut doc, parent, key) {
        return Ok(None);
    }
    if !opts.dry_run {
        atomic_write(path, &doc.to_string())?;
    }
    Ok(Some(Change::Removed(path.to_path_buf())))
}

/// Install a plain file (no marker block, no JSON merge). Idempotent by
/// byte-identical comparison. Used for files we own end-to-end (e.g. the
/// Claude Code `SKILL.md`, where YAML frontmatter forbids comment markers).
pub fn install_plain_file_in(
    path: &Path,
    contents: &str,
    opts: &InstallOpts,
) -> Result<Change, String> {
    let existing = read_optional(path)?;
    if existing.as_deref() == Some(contents) {
        return Ok(Change::Skipped {
            path: path.to_path_buf(),
            reason: "already up to date".into(),
        });
    }
    if !opts.dry_run {
        atomic_write(path, contents)?;
    }
    Ok(if existing.is_some() {
        Change::Updated(path.to_path_buf())
    } else {
        Change::Created(path.to_path_buf())
    })
}

/// Remove a plain file we wrote, but only if its first line still
/// contains `expected_marker` — guards against deleting a file the user
/// has fully replaced with their own content. Also tries to remove the
/// parent directory (succeeds only if empty), keeping `~/.claude/skills/`
/// itself intact when other skills are present.
pub fn uninstall_plain_file_in(
    path: &Path,
    expected_marker: &str,
    opts: &InstallOpts,
) -> Result<Option<Change>, String> {
    let Some(existing) = read_optional(path)? else {
        return Ok(None);
    };
    let first_line = existing.lines().next().unwrap_or("");
    let still_ours = existing.contains(expected_marker) || first_line.contains(expected_marker);
    if !still_ours {
        return Ok(None);
    }
    if !opts.dry_run {
        std::fs::remove_file(path)
            .map_err(|e| format!("remove {}: {}", path.display(), e))?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent); // succeeds only if empty
        }
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
    let mut s = Status::default();
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
    let mut s = Status::default();
    if let Some(pp) = prompt_path {
        if let Ok(Some(contents)) = read_optional(pp) {
            s.prompt_version = marker_block::installed_version(&contents);
            s.prompt_installed = s.prompt_version.is_some();
        }
    }
    s
}
