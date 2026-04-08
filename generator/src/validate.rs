use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::types::FixtureDef;

pub fn validate_definitions(defs_dir: &Path) -> Result<bool> {
    let mut entries: Vec<_> = fs::read_dir(defs_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let total = entries.len();
    let mut valid = 0;

    for entry in &entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        match validate_single(&path) {
            Ok(warnings) => {
                if warnings.is_empty() {
                    println!("  ok  {name}");
                } else {
                    for w in &warnings {
                        println!("  WARN {name}: {w}");
                    }
                }
                valid += 1;
            }
            Err(errors) => {
                for e in &errors {
                    eprintln!("  ERR {name}: {e}");
                }
            }
        }
    }

    println!("\n{valid}/{total} definitions valid");
    Ok(valid == total)
}

fn validate_single(path: &Path) -> std::result::Result<Vec<String>, Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))
        .map_err(|e| vec![e.to_string()])?;

    let def: FixtureDef = serde_json::from_str(&content)
        .with_context(|| "invalid JSON or missing required fields")
        .map_err(|e| vec![e.to_string()])?;

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check meta fields
    if def.meta.name.is_empty() {
        errors.push("meta.name is empty".to_string());
    }
    if def.meta.description.is_empty() {
        errors.push("meta.description is empty".to_string());
    }

    let commit_count = def.commits.len() as i32;

    // Validate tag references
    for tag in &def.tags {
        if tag.at_commit != -1 && (tag.at_commit < 0 || tag.at_commit >= commit_count) {
            errors.push(format!(
                "tag '{}': at_commit {} is out of range (valid: -1..{})",
                tag.name,
                tag.at_commit,
                commit_count - 1
            ));
        }
    }

    // Check for duplicate tag names
    let mut tag_names: Vec<&str> = def.tags.iter().map(|t| t.name.as_str()).collect();
    for pkg in &def.packages {
        if let Some(tag) = &pkg.tag {
            tag_names.push(tag.as_str());
        }
    }
    tag_names.sort();
    for window in tag_names.windows(2) {
        if window[0] == window[1] {
            errors.push(format!("duplicate tag name: '{}'", window[0]));
        }
    }

    // Validate branch references
    let default_branch = def.meta.default_branch.as_deref().unwrap_or("main");
    let branch_names: Vec<&str> = def.branches.iter().map(|b| b.name.as_str()).collect();

    for branch in &def.branches {
        if let Some(at) = branch.at_commit {
            if at != -1 && (at < 0 || at >= commit_count) {
                errors.push(format!(
                    "branch '{}': at_commit {} is out of range (valid: -1..{})",
                    branch.name,
                    at,
                    commit_count - 1
                ));
            }
        }

        if let Some(from) = &branch.from {
            if from != default_branch && !branch_names.contains(&from.as_str()) {
                errors.push(format!(
                    "branch '{}': from '{}' does not reference a known branch",
                    branch.name, from
                ));
            }
        }

        if let Some(merge_into) = &branch.merge {
            if merge_into != default_branch && !branch_names.contains(&merge_into.as_str()) {
                errors.push(format!(
                    "branch '{}': merge target '{}' does not reference a known branch",
                    branch.name, merge_into
                ));
            }
        }
    }

    // Warn about missing expect section
    if def.expect.is_none() {
        warnings
            .push("no 'expect' section — consumers won't have assertions to validate".to_string());
    }

    // Warn if both explicit commits and generate are set
    if def.generate.is_some() && !def.commits.is_empty() {
        warnings.push(
            "both 'generate' and 'commits' are set — 'commits' will be ignored in bulk mode"
                .to_string(),
        );
    }

    if errors.is_empty() {
        Ok(warnings)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_def(dir: &Path, name: &str, content: &str) {
        let path = dir.join(format!("{name}.json"));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn valid_minimal_definition() {
        let tmp = TempDir::new().unwrap();
        write_def(
            tmp.path(),
            "test",
            r#"{"meta":{"name":"test","description":"a test"}}"#,
        );
        assert!(validate_definitions(tmp.path()).unwrap());
    }

    #[test]
    fn invalid_json() {
        let tmp = TempDir::new().unwrap();
        write_def(tmp.path(), "bad", "not json");
        assert!(!validate_definitions(tmp.path()).unwrap());
    }

    #[test]
    fn tag_out_of_range() {
        let tmp = TempDir::new().unwrap();
        write_def(
            tmp.path(),
            "bad-tag",
            r#"{"meta":{"name":"t","description":"d"},"tags":[{"name":"v1","at_commit":5}]}"#,
        );
        assert!(!validate_definitions(tmp.path()).unwrap());
    }

    #[test]
    fn duplicate_tags() {
        let tmp = TempDir::new().unwrap();
        write_def(
            tmp.path(),
            "dup",
            r#"{"meta":{"name":"t","description":"d"},"tags":[{"name":"v1","at_commit":-1},{"name":"v1","at_commit":-1}]}"#,
        );
        assert!(!validate_definitions(tmp.path()).unwrap());
    }

    #[test]
    fn branch_from_unknown() {
        let tmp = TempDir::new().unwrap();
        write_def(
            tmp.path(),
            "bad-branch",
            r#"{"meta":{"name":"t","description":"d"},"branches":[{"name":"feat","from":"nonexistent"}]}"#,
        );
        assert!(!validate_definitions(tmp.path()).unwrap());
    }

    #[test]
    fn empty_meta_name() {
        let tmp = TempDir::new().unwrap();
        write_def(
            tmp.path(),
            "empty",
            r#"{"meta":{"name":"","description":"d"}}"#,
        );
        assert!(!validate_definitions(tmp.path()).unwrap());
    }
}
