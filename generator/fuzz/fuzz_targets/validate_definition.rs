#![no_main]

use generate_fixtures::types::{CommitDef, FixtureDef};
use generate_fixtures::validate;
use libfuzzer_sys::fuzz_target;
use std::path::{Component, Path};

fn stays_inside_root(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn commit_files(commits: &[CommitDef]) -> impl Iterator<Item = &String> {
    commits.iter().flat_map(|c| c.files.iter())
}

fn written_paths(def: &FixtureDef) -> Vec<&str> {
    let mut paths: Vec<&str> = Vec::new();
    paths.extend(commit_files(&def.commits).map(String::as_str));
    for branch in &def.branches {
        paths.extend(commit_files(&branch.commits).map(String::as_str));
    }
    for pkg in &def.packages {
        paths.push(&pkg.path);
        paths.extend(pkg.versioned_files.iter().map(|f| f.path.as_str()));
    }
    paths.extend(def.hooks.iter().map(|h| h.path.as_str()));
    paths.extend(def.config.iter().filter_map(|c| c.filename.as_deref()));
    paths
}

fuzz_target!(|data: &[u8]| {
    let Ok(content) = std::str::from_utf8(data) else {
        return;
    };
    if validate::validate_str(content).is_ok() {
        let def: FixtureDef =
            serde_json::from_str(content).expect("accepted definition must parse");
        for path in written_paths(&def) {
            assert!(stays_inside_root(path), "accepted escaping path {path:?}");
        }
        if let Some(gen) = &def.generate {
            assert_ne!(gen.seed, 0);
        }
    }
});
