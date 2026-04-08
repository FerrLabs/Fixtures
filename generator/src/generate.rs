use anyhow::{Context, Result};
use git2::{Repository, RepositoryInitOptions, Signature};
use std::fs;
use std::path::Path;

use crate::rng::{rand_message, rand_time, Rng};
use crate::tree::BulkRepoBuilder;
use crate::types::{resolve_config_filename, FixtureDef, SerializableExpect};

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
        config.set_str("user.name", "Test")?;
        config.set_str("user.email", "test@test.com")?;
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
            let sig = Signature::now("Test", "test@test.com")?;

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
    let mut rng = Rng::new(gen.seed);
    let now = chrono::Utc::now().timestamp();

    let is_mono = pkg_count > 1;

    if is_mono {
        // Monorepo: generate N packages
        let packages: Vec<String> = (1..=pkg_count).map(|i| format!("pkg-{i:03}")).collect();

        if let Some(config) = &def.config {
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
            let pkg = &packages[rng.usize(pkg_count)];
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

    let sig = Signature::now("Test", "test@test.com")?;

    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit()?],
        Err(_) => vec![],
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;
    Ok(oid)
}

fn create_merge_commit(repo: &Repository, _dir: &Path, message: &str) -> Result<git2::Oid> {
    let sig = Signature::now("Test", "test@test.com")?;
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
