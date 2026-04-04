use anyhow::{Context, Result};
use git2::{Repository, Signature};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct FixtureDef {
    meta: Meta,
    config: ConfigDef,
    #[serde(default)]
    packages: Vec<PackageDef>,
    #[serde(default)]
    commits: Vec<CommitDef>,
    #[serde(default)]
    tags: Vec<TagDef>,
    #[serde(default)]
    hooks: Vec<HookFileDef>,
    expect: ExpectDef,
}

#[derive(Debug, Deserialize)]
struct Meta {
    #[allow(dead_code)]
    name: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct ConfigDef {
    content: String,
    #[serde(default = "default_config_format")]
    format: String,
    #[serde(default)]
    filename: Option<String>,
}

fn default_config_format() -> String {
    "json".to_string()
}

#[derive(Debug, Deserialize)]
struct PackageDef {
    name: String,
    path: String,
    initial_version: String,
    #[serde(default)]
    tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitDef {
    message: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    merge: bool,
}

#[derive(Debug, Deserialize)]
struct TagDef {
    name: String,
    at_commit: i32,
}

#[derive(Debug, Deserialize)]
struct HookFileDef {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ExpectDef {
    #[serde(default)]
    check_contains: Vec<String>,
    #[serde(default)]
    check_not_contains: Vec<String>,
    #[serde(default)]
    output_order: Vec<String>,
    #[serde(default)]
    packages_released: Option<usize>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (defs_dir, gen_dir) = parse_args(&args)?;

    if !defs_dir.exists() {
        anyhow::bail!("{} not found.", defs_dir.display());
    }

    // Clean generated directory
    if gen_dir.exists() {
        fs::remove_dir_all(&gen_dir)?;
    }
    fs::create_dir_all(&gen_dir)?;

    let mut entries: Vec<_> = fs::read_dir(&defs_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let total = entries.len();
    let mut ok = 0;

    for entry in &entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        match generate_fixture(&path, &gen_dir.join(&name)) {
            Ok(()) => {
                println!("  ok  {name}");
                ok += 1;
            }
            Err(e) => {
                eprintln!("  ERR {name}: {e}");
            }
        }
    }

    println!("\n{ok}/{total} fixtures generated");
    if ok < total {
        std::process::exit(1);
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(PathBuf, PathBuf)> {
    let mut defs_dir = PathBuf::from("fixtures/definitions");
    let mut gen_dir = PathBuf::from("fixtures/generated");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--definitions" | "-d" => {
                i += 1;
                defs_dir = PathBuf::from(args.get(i).context("missing value for --definitions")?);
            }
            "--output" | "-o" => {
                i += 1;
                gen_dir = PathBuf::from(args.get(i).context("missing value for --output")?);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
        i += 1;
    }

    Ok((defs_dir, gen_dir))
}

fn generate_fixture(def_path: &Path, output_dir: &Path) -> Result<()> {
    let content = fs::read_to_string(def_path)
        .with_context(|| format!("reading {}", def_path.display()))?;
    let def: FixtureDef =
        toml::from_str(&content).with_context(|| format!("parsing {}", def_path.display()))?;

    fs::create_dir_all(output_dir)?;

    // Init git repo
    let repo = Repository::init(output_dir)?;
    {
        let mut config = repo.config()?;
        config.set_str("user.name", "Test")?;
        config.set_str("user.email", "test@test.com")?;
    }

    // Write ferrflow config with the specified format and filename
    let config_filename = def.config.filename.clone().unwrap_or_else(|| {
        match def.config.format.as_str() {
            "toml" => ".ferrflow.toml".to_string(),
            "json5" => "ferrflow.json5".to_string(),
            _ => "ferrflow.json".to_string(),
        }
    });
    fs::write(output_dir.join(&config_filename), &def.config.content)?;

    // Write version files for each package
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

    // Write hook scripts
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

    // Initial commit
    let initial_oid = add_all_and_commit(&repo, output_dir, "chore: initial setup")?;
    let mut commit_oids: Vec<git2::Oid> = Vec::new();

    // Create tags from old-style PackageDef.tag (placed on initial commit)
    for pkg in &def.packages {
        if let Some(tag) = &pkg.tag {
            let commit = repo.find_commit(initial_oid)?;
            repo.tag_lightweight(tag, commit.as_object(), false)?;
        }
    }

    // Apply tags at initial commit (at_commit == -1)
    for tag_def in &def.tags {
        if tag_def.at_commit == -1 {
            let commit = repo.find_commit(initial_oid)?;
            repo.tag_lightweight(&tag_def.name, commit.as_object(), false)?;
        }
    }

    // Apply commits and collect OIDs
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
            add_all_and_commit(&repo, output_dir, &commit_def.message)?
        };
        commit_oids.push(oid);

        // Apply tags at this commit index
        for tag_def in &def.tags {
            if tag_def.at_commit == i as i32 {
                let tagged_commit = repo.find_commit(oid)?;
                repo.tag_lightweight(&tag_def.name, tagged_commit.as_object(), false)?;
            }
        }
    }

    // Write expect metadata for the runner
    let expect_path = output_dir.join(".expect.toml");
    let expect_content = toml::to_string_pretty(&SerializableExpect {
        check_contains: &def.expect.check_contains,
        check_not_contains: &def.expect.check_not_contains,
        output_order: &def.expect.output_order,
        packages_released: def.expect.packages_released,
        description: &def.meta.description,
    })?;
    fs::write(&expect_path, expect_content)?;

    Ok(())
}

fn add_all_and_commit(repo: &Repository, _dir: &Path, message: &str) -> Result<git2::Oid> {
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

    // Stage working changes
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Create the branch commit (not updating HEAD)
    let branch_oid = repo.commit(
        None,
        &sig,
        &sig,
        &format!("{message} (branch)"),
        &tree,
        &[&main_commit],
    )?;
    let branch_commit = repo.find_commit(branch_oid)?;

    // Create the merge commit with two parents
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

#[derive(serde::Serialize)]
struct SerializableExpect<'a> {
    description: &'a str,
    check_contains: &'a [String],
    check_not_contains: &'a [String],
    output_order: &'a [String],
    packages_released: Option<usize>,
}
