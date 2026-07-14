use anyhow::{Context, Result};
use git2::{Repository, RepositoryInitOptions, Signature};
use std::fs;
use std::path::Path;

use crate::rng::{rand_message, rand_time, Rng};
use crate::tree::BulkRepoBuilder;
use crate::types::{resolve_config_filename, FixtureDef, SerializableExpect};

pub const AUTHOR_NAME: &str = "Test";
pub const AUTHOR_EMAIL: &str = "test@test.com";

pub fn generate_fixture(def_path: &Path, output_dir: &Path, verbose: bool) -> Result<()> {
    let content =
        fs::read_to_string(def_path).with_context(|| format!("reading {}", def_path.display()))?;
    let def: FixtureDef = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", def_path.display()))?;

    if verbose {
        let mode = if def.generate.is_some() {
            "bulk"
        } else {
            "explicit"
        };
        eprintln!(
            "        mode={mode}, packages={}, commits={}, tags={}, branches={}, hooks={}",
            def.packages.len(),
            def.commits.len(),
            def.tags.len(),
            def.branches.len(),
            def.hooks.len(),
        );
    }

    if def.generate.is_some() {
        generate_bulk(&def, output_dir, verbose)
    } else {
        generate_explicit(&def, output_dir, verbose)
    }
}

/// Generate a fixture from explicit [[commits]] definitions.
fn generate_explicit(def: &FixtureDef, output_dir: &Path, verbose: bool) -> Result<()> {
    fs::create_dir_all(output_dir)?;

    let repo = init_repo(output_dir, def.meta.default_branch.as_deref())?;
    {
        let mut config = repo.config()?;
        config.set_str("user.name", AUTHOR_NAME)?;
        config.set_str("user.email", AUTHOR_EMAIL)?;
    }

    if let Some(config) = &def.config {
        let config_filename = resolve_config_filename(config);
        if verbose {
            eprintln!("        config -> {config_filename}");
        }
        fs::write(output_dir.join(&config_filename), &config.content)?;
    }

    for pkg in &def.packages {
        let pkg_dir = output_dir.join(&pkg.path);
        fs::create_dir_all(pkg_dir.join("src"))?;

        let version_file = if pkg.path == "." {
            output_dir.join("version.toml")
        } else {
            output_dir.join(&pkg.path).join("version.toml")
        };
        fs::write(
            &version_file,
            format!(
                "[package]\nname = \"{}\"\nversion = \"{}\"\n",
                pkg.name, pkg.initial_version
            ),
        )?;
    }

    for hook in &def.hooks {
        let hook_path = output_dir.join(&hook.path);
        if let Some(parent) = hook_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&hook_path, &hook.content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
        }
    }

    let initial_oid = add_all_and_commit(&repo, "chore: initial setup")?;
    let mut commit_oids: Vec<git2::Oid> = Vec::new();

    for pkg in &def.packages {
        if let Some(tag) = &pkg.tag {
            let commit = repo.find_commit(initial_oid)?;
            repo.tag_lightweight(tag, commit.as_object(), false)?;
        }
    }

    for tag_def in &def.tags {
        if tag_def.at_commit == -1 {
            let commit = repo.find_commit(initial_oid)?;
            repo.tag_lightweight(&tag_def.name, commit.as_object(), false)?;
        }
    }

    for (i, commit_def) in def.commits.iter().enumerate() {
        for file in &commit_def.files {
            let file_path = output_dir.join(file);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, format!("// generated content for {file}\n"))?;
        }

        let oid = if commit_def.merge {
            create_merge_commit(&repo, output_dir, &commit_def.message)?
        } else {
            add_all_and_commit(&repo, &commit_def.message)?
        };
        if verbose {
            let kind = if commit_def.merge { "merge" } else { "commit" };
            eprintln!("        {kind} [{i}]: {}", commit_def.message);
        }
        commit_oids.push(oid);

        for tag_def in &def.tags {
            if tag_def.at_commit == i as i32 {
                let tagged_commit = repo.find_commit(oid)?;
                repo.tag_lightweight(&tag_def.name, tagged_commit.as_object(), false)?;
                if verbose {
                    eprintln!("        tag: {}", tag_def.name);
                }
            }
        }
    }

    // Process branches: create each branch from a specific point and add commits.
    let default_branch_name = def.meta.default_branch.as_deref().unwrap_or("main");

    for branch_def in &def.branches {
        // Resolve the fork point: find the commit to branch from.
        let from_branch = branch_def.from.as_deref().unwrap_or(default_branch_name);
        let fork_oid = match branch_def.at_commit {
            Some(-1) => initial_oid,
            Some(idx) if idx >= 0 => *commit_oids.get(idx as usize).ok_or_else(|| {
                anyhow::anyhow!(
                    "branch '{}': at_commit {} out of range (only {} commits)",
                    branch_def.name,
                    idx,
                    commit_oids.len()
                )
            })?,
            _ => {
                // Default: tip of the from-branch
                let reference = repo
                    .find_branch(from_branch, git2::BranchType::Local)
                    .with_context(|| {
                        format!(
                            "branch '{}': source branch '{}' not found",
                            branch_def.name, from_branch
                        )
                    })?;
                reference.get().peel_to_commit()?.id()
            }
        };

        // Create the branch ref at the fork point.
        let fork_commit = repo.find_commit(fork_oid)?;
        repo.branch(&branch_def.name, &fork_commit, false)?;
        if verbose {
            eprintln!("        branch: {} (from {from_branch})", branch_def.name);
        }

        // Switch HEAD to the new branch to add commits.
        repo.set_head(&format!("refs/heads/{}", branch_def.name))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;

        // Add commits on this branch.
        for commit_def in &branch_def.commits {
            for file in &commit_def.files {
                let file_path = output_dir.join(file);
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&file_path, format!("// generated content for {file}\n"))?;
            }

            if commit_def.merge {
                create_merge_commit(&repo, output_dir, &commit_def.message)?;
            } else {
                add_all_and_commit(&repo, &commit_def.message)?;
            };
        }

        // Merge back into target branch if requested.
        if let Some(merge_into) = &branch_def.merge {
            let branch_tip = repo.head()?.peel_to_commit()?;

            // Switch to the target branch.
            repo.set_head(&format!("refs/heads/{}", merge_into))?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;

            let target_tip = repo.head()?.peel_to_commit()?;

            // Create merge commit with both parents.
            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;
            let tree_id = index.write_tree()?;
            let tree = repo.find_tree(tree_id)?;
            let sig = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)?;

            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("Merge branch '{}'", branch_def.name),
                &tree,
                &[&target_tip, &branch_tip],
            )?;
        }
    }

    // Restore HEAD to the default branch.
    if !def.branches.is_empty() {
        repo.set_head(&format!("refs/heads/{}", default_branch_name))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    }

    write_expect(output_dir, def)?;
    Ok(())
}

/// Generate a fixture with bulk synthetic commits (fast incremental tree builder).
fn generate_bulk(def: &FixtureDef, output_dir: &Path, verbose: bool) -> Result<()> {
    let gen = def.generate.as_ref().unwrap();
    let pkg_count = gen.packages;
    let commit_count = gen.commits;

    fs::create_dir_all(output_dir)?;

    let repo = init_repo(output_dir, def.meta.default_branch.as_deref())?;
    let mut b = BulkRepoBuilder::new();
    let mut rng = Rng::new(gen.seed)?;
    let now = chrono::Utc::now().timestamp();

    let is_mono = pkg_count > 1;

    if is_mono {
        // Monorepo: generate N packages
        let packages: Vec<String> = (1..=pkg_count).map(|i| format!("pkg-{i:03}")).collect();
        let core_count = (pkg_count / 5).max(1);

        if gen.graph {
            let deps = build_dependency_edges(pkg_count, core_count, &mut rng);
            b.set_file(
                &repo,
                "ferrflow.json",
                graph_config(&packages, &deps).as_bytes(),
            )?;
        } else if let Some(config) = &def.config {
            b.set_file(
                &repo,
                &resolve_config_filename(config),
                config.content.as_bytes(),
            )?;
        }

        for p in &packages {
            let content = format!("{{\n  \"name\": \"{p}\",\n  \"version\": \"0.1.0\"\n}}\n");
            b.set_file(
                &repo,
                &format!("packages/{p}/package.json"),
                content.as_bytes(),
            )?;
        }

        let t = rand_time(&mut rng, now);
        let oid = b.commit(&repo, None, "chore: initial commit", &t)?;

        let obj = repo.find_object(oid, None)?;
        for p in &packages {
            repo.tag_lightweight(&format!("{p}@v0.1.0"), &obj, false)?;
        }

        let mut parent = oid;
        for i in 1..=commit_count {
            let idx = if gen.graph {
                rng.usize(core_count)
            } else {
                rng.usize(pkg_count)
            };
            let pkg = &packages[idx];
            let path = format!("packages/{pkg}/dummy.txt");
            b.append_dummy(&repo, &path)?;

            let msg = rand_message(&mut rng, pkg);
            let t = rand_time(&mut rng, now);
            parent = b.commit(&repo, Some(parent), &msg, &t)?;

            if verbose && i % 500 == 0 {
                eprintln!("        {i}/{commit_count} commits");
            } else if commit_count >= 1000 && i % 2000 == 0 {
                eprintln!("    {}/{commit_count}", i);
            }
        }
    } else {
        // Single package
        if let Some(config) = &def.config {
            b.set_file(
                &repo,
                &resolve_config_filename(config),
                config.content.as_bytes(),
            )?;
        }
        b.set_file(
            &repo,
            "package.json",
            b"{\n  \"name\": \"myapp\",\n  \"version\": \"0.1.0\"\n}\n",
        )?;
        b.set_file(&repo, "dummy.txt", b"")?;

        let t = rand_time(&mut rng, now);
        let oid = b.commit(&repo, None, "chore: initial commit", &t)?;

        let obj = repo.find_object(oid, None)?;
        repo.tag_lightweight("v0.1.0", &obj, false)?;

        let scopes = ["core", "api", "cli", "config", "parser"];
        let mut parent = oid;
        for _ in 0..commit_count {
            b.append_dummy(&repo, "dummy.txt")?;
            let scope = rng.pick(&scopes);
            let msg = rand_message(&mut rng, scope);
            let t = rand_time(&mut rng, now);
            parent = b.commit(&repo, Some(parent), &msg, &t)?;
        }
    }

    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    write_expect(output_dir, def)?;
    Ok(())
}

/// Wire `pkg_count` packages into a DAG: the first `core_count` packages form a
/// spine (each depends on the previous), and every remaining leaf depends on 1-3
/// core packages. Deterministic for a given `rng`. Returns per-package dependency
/// indices into the package list.
fn build_dependency_edges(pkg_count: usize, core_count: usize, rng: &mut Rng) -> Vec<Vec<usize>> {
    let mut deps = vec![Vec::new(); pkg_count];
    for (i, edges) in deps.iter_mut().enumerate().take(core_count).skip(1) {
        edges.push(i - 1);
    }
    let max_deps = 3.min(core_count);
    for edges in deps.iter_mut().skip(core_count) {
        let n = 1 + rng.usize(max_deps);
        let mut chosen = std::collections::BTreeSet::new();
        while chosen.len() < n {
            chosen.insert(rng.usize(core_count));
        }
        *edges = chosen.into_iter().collect();
    }
    deps
}

/// Render a ferrflow config wiring each package to its package.json and emitting
/// `dependsOn` edges for packages that have dependencies. `recoverMissedReleases`
/// is on so that every package with commits since its tag is considered (not just
/// the one touched by HEAD), which is what lets the dependency cascade fire.
fn graph_config(packages: &[String], deps: &[Vec<usize>]) -> String {
    let entries: Vec<String> = packages
        .iter()
        .zip(deps)
        .map(|(name, edges)| {
            let mut entry = format!(
                "{{\"name\":\"{name}\",\"path\":\"packages/{name}\",\"versioned_files\":[{{\"path\":\"packages/{name}/package.json\",\"format\":\"json\"}}]"
            );
            if !edges.is_empty() {
                let names: Vec<String> =
                    edges.iter().map(|&d| format!("\"{}\"", packages[d])).collect();
                entry.push_str(&format!(",\"dependsOn\":[{}]", names.join(",")));
            }
            entry.push('}');
            entry
        })
        .collect();
    format!(
        "{{\"workspace\":{{\"recoverMissedReleases\":true}},\"package\":[{}]}}",
        entries.join(",")
    )
}

fn init_repo(path: &Path, default_branch: Option<&str>) -> Result<Repository> {
    match default_branch {
        Some(branch) => {
            let mut opts = RepositoryInitOptions::new();
            opts.initial_head(branch);
            Ok(Repository::init_opts(path, &opts)?)
        }
        None => Ok(Repository::init(path)?),
    }
}

fn add_all_and_commit(repo: &Repository, message: &str) -> Result<git2::Oid> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let sig = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)?;

    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit()?],
        Err(_) => vec![],
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;
    Ok(oid)
}

fn create_merge_commit(repo: &Repository, _dir: &Path, message: &str) -> Result<git2::Oid> {
    let sig = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)?;
    let main_commit = repo.head()?.peel_to_commit()?;

    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let branch_oid = repo.commit(
        None,
        &sig,
        &sig,
        &format!("{message} (branch)"),
        &tree,
        &[&main_commit],
    )?;
    let branch_commit = repo.find_commit(branch_oid)?;

    let merge_oid = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &format!("Merge branch: {message}"),
        &tree,
        &[&main_commit, &branch_commit],
    )?;

    Ok(merge_oid)
}

fn write_expect(output_dir: &Path, def: &FixtureDef) -> Result<()> {
    let expect = def.expect.as_ref();
    let expect_path = output_dir.join(".expect.toml");
    let expect_content = toml::to_string_pretty(&SerializableExpect {
        check_contains: expect.map(|e| &e.check_contains[..]).unwrap_or(&[]),
        check_not_contains: expect.map(|e| &e.check_not_contains[..]).unwrap_or(&[]),
        output_order: expect.map(|e| &e.output_order[..]).unwrap_or(&[]),
        packages_released: expect.and_then(|e| e.packages_released),
        description: &def.meta.description,
    })?;
    fs::write(&expect_path, expect_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_def(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(format!("{name}.json"));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn count_commits(repo: &Repository) -> usize {
        let mut revwalk = repo.revwalk().unwrap();
        revwalk.push_head().unwrap();
        revwalk.count()
    }

    fn get_tags(repo: &Repository) -> Vec<String> {
        let mut tags = Vec::new();
        repo.tag_foreach(|_oid, name| {
            let name = std::str::from_utf8(name).unwrap();
            let short = name.strip_prefix("refs/tags/").unwrap_or(name);
            tags.push(short.to_string());
            true
        })
        .unwrap();
        tags.sort();
        tags
    }

    #[test]
    fn generates_valid_git_repo() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"basic"},"packages":[{"name":"app","path":".","initial_version":"1.0.0","tag":"v1.0.0"}],"commits":[{"message":"feat: add feature","files":["src/main.rs"]}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        assert!(repo.head().is_ok());
    }

    #[test]
    fn correct_commit_count() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"commits":[{"message":"feat: one","files":["a.txt"]},{"message":"fix: two","files":["b.txt"]},{"message":"chore: three","files":["c.txt"]}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        // 3 explicit commits + 1 initial setup = 4
        assert_eq!(count_commits(&repo), 4);
    }

    #[test]
    fn tags_are_created() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"packages":[{"name":"app","path":".","initial_version":"1.0.0","tag":"v1.0.0"}],"commits":[{"message":"feat: bump","files":["a.txt"]}],"tags":[{"name":"v1.1.0","at_commit":0}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        let tags = get_tags(&repo);
        assert!(tags.contains(&"v1.0.0".to_string()));
        assert!(tags.contains(&"v1.1.0".to_string()));
    }

    #[test]
    fn config_file_is_written() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"config":{"content":"{\"package\":[]}","format":"json"}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let config_path = out.join("ferrflow.json");
        assert!(config_path.exists());
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("package"));
    }

    #[test]
    fn toml_config_format() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"config":{"content":"[package]","format":"toml"}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        assert!(out.join(".ferrflow.toml").exists());
    }

    #[test]
    fn expect_toml_is_generated() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"my description"},"expect":{"check_contains":["hello"],"packages_released":1}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let expect_path = out.join(".expect.toml");
        assert!(expect_path.exists());
        let content = fs::read_to_string(&expect_path).unwrap();
        assert!(content.contains("my description"));
        assert!(content.contains("hello"));
        assert!(content.contains("packages_released = 1"));
    }

    #[test]
    fn hook_files_are_created() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"hooks":[{"path":"hooks/pre-bump.sh","content":"echo hi"}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let hook_path = out.join("hooks/pre-bump.sh");
        assert!(hook_path.exists());
        let content = fs::read_to_string(&hook_path).unwrap();
        assert_eq!(content, "echo hi");
    }

    #[test]
    fn branches_are_created() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d","default_branch":"main"},"commits":[{"message":"feat: base","files":["a.txt"]}],"branches":[{"name":"develop","at_commit":0,"commits":[{"message":"feat: dev work","files":["b.txt"]}]}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        let branch = repo.find_branch("develop", git2::BranchType::Local);
        assert!(branch.is_ok(), "develop branch should exist");
    }

    #[test]
    fn branch_merge_creates_merge_commit() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d","default_branch":"main"},"commits":[{"message":"feat: base","files":["a.txt"]}],"branches":[{"name":"feature","at_commit":0,"merge":"main","commits":[{"message":"feat: on branch","files":["b.txt"]}]}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        // Merge commit has 2 parents
        assert_eq!(head.parent_count(), 2);
    }

    #[test]
    fn bulk_generation_single_package() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"config":{"content":"{}"},"generate":{"packages":1,"commits":10,"seed":42}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        // 10 commits + 1 initial = 11
        assert_eq!(count_commits(&repo), 11);

        let tags = get_tags(&repo);
        assert!(tags.contains(&"v0.1.0".to_string()));
    }

    #[test]
    fn bulk_generation_monorepo() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"config":{"content":"{}"},"generate":{"packages":3,"commits":5,"seed":1}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        // 5 commits + 1 initial = 6
        assert_eq!(count_commits(&repo), 6);

        let tags = get_tags(&repo);
        assert!(tags.contains(&"pkg-001@v0.1.0".to_string()));
        assert!(tags.contains(&"pkg-002@v0.1.0".to_string()));
        assert!(tags.contains(&"pkg-003@v0.1.0".to_string()));
    }

    #[test]
    fn custom_default_branch() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d","default_branch":"master"}}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "master",);
    }

    #[test]
    fn invalid_json_returns_error() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(def_dir.path(), "bad", "not json");

        let out = tmp.path().join("bad");
        let result = generate_fixture(&def_path, &out, false);
        assert!(result.is_err());
    }

    #[test]
    fn merge_commit_in_main_branch() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "test",
            r#"{"meta":{"name":"test","description":"d"},"commits":[{"message":"feat: merged feature","files":["src/feature.rs"],"merge":true}]}"#,
        );

        let out = tmp.path().join("test");
        generate_fixture(&def_path, &out, false).unwrap();

        let repo = Repository::open(&out).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        // The merge commit at HEAD should have 2 parents
        assert_eq!(head.parent_count(), 2);
    }

    #[test]
    fn dependency_edges_form_a_dag() {
        let core_count = 3;
        let mut rng = Rng::new(20250714).unwrap();
        let deps = build_dependency_edges(12, core_count, &mut rng);

        assert_eq!(deps.len(), 12);
        assert!(deps[0].is_empty());
        assert_eq!(deps[1], vec![0]);
        assert_eq!(deps[2], vec![1]);
        for (i, edges) in deps.iter().enumerate().skip(core_count) {
            assert!(!edges.is_empty(), "leaf {i} has no dependencies");
            for &d in edges {
                assert!(d < core_count, "leaf {i} depends on non-core {d}");
                assert_ne!(d, i, "leaf {i} depends on itself");
            }
        }

        let mut rng2 = Rng::new(20250714).unwrap();
        assert_eq!(deps, build_dependency_edges(12, core_count, &mut rng2));
    }

    #[test]
    fn graph_mode_writes_depends_on_config() {
        let tmp = TempDir::new().unwrap();
        let def_dir = TempDir::new().unwrap();
        let def_path = write_def(
            def_dir.path(),
            "graph",
            r#"{"meta":{"name":"graph","description":"d"},"generate":{"packages":10,"commits":5,"seed":7,"graph":true}}"#,
        );

        let out = tmp.path().join("graph");
        generate_fixture(&def_path, &out, false).unwrap();

        let config = fs::read_to_string(out.join("ferrflow.json")).unwrap();
        assert!(
            config.contains("\"dependsOn\""),
            "config lacks edges: {config}"
        );
        assert!(config.contains("\"recoverMissedReleases\":true"));
        assert!(config.contains("pkg-001"));
        assert!(config.contains("pkg-010"));
        assert!(out.join("packages/pkg-001/package.json").exists());
    }
}
