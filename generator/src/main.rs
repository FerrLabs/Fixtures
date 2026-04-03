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
    let defs_dir = PathBuf::from("fixtures/definitions");
    let gen_dir = PathBuf::from("fixtures/generated");

    if !defs_dir.exists() {
        anyhow::bail!("fixtures/definitions/ not found. Run from the repo root.");
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

    // Write ferrflow config
    fs::write(output_dir.join("ferrflow.json"), &def.config.content)?;

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

    // Initial commit
    add_all_and_commit(&repo, output_dir, "chore: initial setup")?;

    // Create initial tags
    for pkg in &def.packages {
        if let Some(tag) = &pkg.tag {
            let head = repo.head()?.peel_to_commit()?;
            repo.tag_lightweight(tag, head.as_object(), false)?;
        }
    }

    // Apply commits
    for commit in &def.commits {
        for file in &commit.files {
            let file_path = output_dir.join(file);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, format!("// generated content for {file}\n"))?;
        }
        add_all_and_commit(&repo, output_dir, &commit.message)?;
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

#[derive(serde::Serialize)]
struct SerializableExpect<'a> {
    description: &'a str,
    check_contains: &'a [String],
    check_not_contains: &'a [String],
    output_order: &'a [String],
    packages_released: Option<usize>,
}
