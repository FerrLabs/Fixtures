use std::fs;
use std::path::Path;
use std::process::Command;

fn loose_object_count(repo_dir: &Path) -> usize {
    let objects = repo_dir.join(".git").join("objects");
    let mut count = 0;
    let Ok(entries) = fs::read_dir(&objects) else {
        return 0;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(inner) = fs::read_dir(entry.path()) {
            count += inner.flatten().count();
        }
    }
    count
}

#[test]
fn packing_ignores_an_inherited_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let defs = tmp.path().join("defs");
    fs::create_dir_all(&defs).unwrap();
    fs::write(
        defs.join("packed.json"),
        r#"{"meta":{"name":"packed","description":"packed","default_branch":"main"},"generate":{"packages":1,"commits":5}}"#,
    )
    .unwrap();

    let decoy = tmp.path().join("decoy");
    fs::create_dir_all(&decoy).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&decoy)
            // Without this the decoy is never built: `git init` honours an
            // ambient GIT_DIR over the working directory, so under a hook it
            // would reinitialise the repository being pushed instead.
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .status()
            .unwrap()
            .success(),
        "could not create the decoy repository"
    );
    assert!(
        decoy.join(".git").is_dir(),
        "the decoy was not built, so GIT_DIR captured the init"
    );

    let out = tmp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_generate-fixtures"))
        .arg("--definitions")
        .arg(&defs)
        .arg("--output")
        .arg(&out)
        .env("GIT_DIR", decoy.join(".git"))
        .status()
        .unwrap();

    assert!(status.success(), "generation failed with GIT_DIR set");
    assert_eq!(
        loose_object_count(&out.join("packed")),
        0,
        "GIT_DIR leaked into the pack, so the fixture kept its loose objects"
    );
}
