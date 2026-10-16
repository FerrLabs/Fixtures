use std::fs;
use std::process::Command;

#[test]
fn package_versioned_files_are_written_and_committed() {
    let tmp = tempfile::tempdir().unwrap();
    let defs = tmp.path().join("defs");
    fs::create_dir_all(&defs).unwrap();
    fs::write(
        defs.join("multi.json"),
        r#"{"meta":{"name":"multi","description":"d","default_branch":"main"},"packages":[{"name":"m","path":".","initial_version":"1.0.0","versioned_files":[{"path":"go.mod","format":"gomod","content":"module example\n"},{"path":"nested/version2.toml","content":"v = 1\n"},{"path":"skipped.txt"}]}]}"#,
    )
    .unwrap();

    let out = tmp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_generate-fixtures"))
        .arg("--definitions")
        .arg(&defs)
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());

    let repo = out.join("multi");
    assert_eq!(
        fs::read_to_string(repo.join("go.mod")).unwrap(),
        "module example\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("nested/version2.toml")).unwrap(),
        "v = 1\n"
    );
    assert!(repo.join("version.toml").exists());
    assert!(!repo.join("skipped.txt").exists());

    let tracked = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(&repo)
        .env_remove("GIT_DIR")
        .output()
        .unwrap();
    let tracked = String::from_utf8(tracked.stdout).unwrap();
    assert!(tracked.lines().any(|l| l == "go.mod"));
    assert!(tracked.lines().any(|l| l == "nested/version2.toml"));
}
