use anyhow::{Context, Result};
use git2::{FileMode, Oid, Repository, RepositoryInitOptions, Signature, Time};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Definition types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FixtureDef {
    meta: Meta,
    #[serde(default)]
    config: Option<ConfigDef>,
    #[serde(default)]
    packages: Vec<PackageDef>,
    #[serde(default)]
    commits: Vec<CommitDef>,
    #[serde(default)]
    tags: Vec<TagDef>,
    #[serde(default)]
    branches: Vec<BranchDef>,
    #[serde(default)]
    hooks: Vec<HookFileDef>,
    #[serde(default)]
    generate: Option<GenerateDef>,
    #[serde(default)]
    expect: Option<ExpectDef>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    #[allow(dead_code)]
    name: String,
    description: String,
    #[serde(default)]
    default_branch: Option<String>,
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
struct BranchDef {
    name: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    at_commit: Option<i32>,
    #[serde(default)]
    commits: Vec<CommitDef>,
    #[serde(default)]
    merge: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HookFileDef {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct GenerateDef {
    #[serde(default = "default_gen_packages")]
    packages: usize,
    #[serde(default = "default_gen_commits")]
    commits: usize,
    #[serde(default = "default_gen_seed")]
    seed: u64,
}

fn default_gen_packages() -> usize {
    1
}
fn default_gen_commits() -> usize {
    100
}
fn default_gen_seed() -> u64 {
    42
}

#[derive(Debug, Deserialize, Default)]
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

// ---------------------------------------------------------------------------
// Incremental tree builder for bulk commits (fast)
// ---------------------------------------------------------------------------

enum TreeEntry {
    Blob(Oid),
    Tree(TreeNode),
}

struct TreeNode {
    entries: HashMap<String, TreeEntry>,
    cached_oid: Option<Oid>,
}

impl TreeNode {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            cached_oid: None,
        }
    }

    fn invalidate(&mut self) {
        self.cached_oid = None;
    }

    fn insert_blob(&mut self, path: &str, blob_oid: Oid) {
        self.invalidate();
        if let Some(slash) = path.find('/') {
            let dir = &path[..slash];
            let rest = &path[slash + 1..];
            let child = self
                .entries
                .entry(dir.to_string())
                .or_insert_with(|| TreeEntry::Tree(TreeNode::new()));
            match child {
                TreeEntry::Tree(node) => node.insert_blob(rest, blob_oid),
                _ => panic!("path conflict: {dir} is a blob, not a tree"),
            }
        } else {
            self.entries
                .insert(path.to_string(), TreeEntry::Blob(blob_oid));
        }
    }

    fn write(&mut self, repo: &Repository) -> Result<Oid> {
        if let Some(oid) = self.cached_oid {
            return Ok(oid);
        }
        let mut builder = repo.treebuilder(None)?;
        for (name, entry) in &mut self.entries {
            match entry {
                TreeEntry::Blob(oid) => {
                    builder.insert(name, *oid, FileMode::Blob.into())?;
                }
                TreeEntry::Tree(node) => {
                    let oid = node.write(repo)?;
                    builder.insert(name, oid, FileMode::Tree.into())?;
                }
            }
        }
        let oid = builder.write()?;
        self.cached_oid = Some(oid);
        Ok(oid)
    }
}

struct BulkRepoBuilder {
    root: TreeNode,
    dummy_content: HashMap<String, Vec<u8>>,
}

impl BulkRepoBuilder {
    fn new() -> Self {
        Self {
            root: TreeNode::new(),
            dummy_content: HashMap::new(),
        }
    }

    fn set_file(&mut self, repo: &Repository, path: &str, content: &[u8]) -> Result<()> {
        let blob_oid = repo.blob(content)?;
        self.root.insert_blob(path, blob_oid);
        Ok(())
    }

    fn append_dummy(&mut self, repo: &Repository, path: &str) -> Result<()> {
        let content = self.dummy_content.entry(path.to_string()).or_default();
        content.extend_from_slice(b"change\n");
        let blob_oid = repo.blob(content)?;
        self.root.insert_blob(path, blob_oid);
        Ok(())
    }

    fn commit(
        &mut self,
        repo: &Repository,
        parent: Option<Oid>,
        msg: &str,
        time: &Time,
    ) -> Result<Oid> {
        let tree_id = self.root.write(repo)?;
        let tree = repo.find_tree(tree_id)?;
        let s = Signature::new("Test", "test@test.com", time)?;

        let oid = match parent {
            Some(pid) => {
                let p = repo.find_commit(pid)?;
                repo.commit(Some("HEAD"), &s, &s, msg, &tree, &[&p])?
            }
            None => repo.commit(Some("HEAD"), &s, &s, msg, &tree, &[])?,
        };
        Ok(oid)
    }
}

// ---------------------------------------------------------------------------
// Simple RNG for deterministic bulk generation
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn usize(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }

    fn pick<'a>(&mut self, items: &'a [&str]) -> &'a str {
        items[self.usize(items.len())]
    }
}

const COMMIT_TYPES: &[&str] = &[
    "feat", "fix", "refactor", "perf", "chore", "docs", "ci", "test",
];
const WORDS_A: &[&str] = &[
    "update",
    "add",
    "remove",
    "refactor",
    "improve",
    "fix",
    "handle",
    "support",
    "implement",
    "optimize",
];
const WORDS_B: &[&str] = &[
    "feature",
    "endpoint",
    "handler",
    "logic",
    "validation",
    "error",
    "check",
    "flow",
    "config",
    "output",
];

fn rand_message(rng: &mut Rng, scope: &str) -> String {
    let t = rng.pick(COMMIT_TYPES);
    let bang = if rng.usize(20) == 0 { "!" } else { "" };
    let a = rng.pick(WORDS_A);
    let b = rng.pick(WORDS_B);
    format!("{t}({scope}){bang}: {a} {b}")
}

fn rand_time(rng: &mut Rng, now: i64) -> Time {
    let days = rng.usize(365) as i64;
    let hours = rng.usize(24) as i64;
    let mins = rng.usize(60) as i64;
    let offset = days * 86400 + hours * 3600 + mins * 60;
    Time::new(now - offset, 0)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (defs_dir, gen_dir) = parse_args(&args)?;

    if !defs_dir.exists() {
        anyhow::bail!("{} not found.", defs_dir.display());
    }

    if gen_dir.exists() {
        fs::remove_dir_all(&gen_dir)?;
    }
    fs::create_dir_all(&gen_dir)?;

    let mut entries: Vec<_> = fs::read_dir(&defs_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
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

// ---------------------------------------------------------------------------
// Fixture generation
// ---------------------------------------------------------------------------

fn generate_fixture(def_path: &Path, output_dir: &Path) -> Result<()> {
    let content =
        fs::read_to_string(def_path).with_context(|| format!("reading {}", def_path.display()))?;
    let def: FixtureDef = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", def_path.display()))?;

    if def.generate.is_some() {
        generate_bulk(&def, output_dir)
    } else {
        generate_explicit(&def, output_dir)
    }
}

/// Generate a fixture from explicit [[commits]] definitions.
fn generate_explicit(def: &FixtureDef, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;

    let repo = init_repo(output_dir, def.meta.default_branch.as_deref())?;
    {
        let mut config = repo.config()?;
        config.set_str("user.name", "Test")?;
        config.set_str("user.email", "test@test.com")?;
    }

    if let Some(config) = &def.config {
        let config_filename = resolve_config_filename(config);
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
        commit_oids.push(oid);

        for tag_def in &def.tags {
            if tag_def.at_commit == i as i32 {
                let tagged_commit = repo.find_commit(oid)?;
                repo.tag_lightweight(&tag_def.name, tagged_commit.as_object(), false)?;
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
fn generate_bulk(def: &FixtureDef, output_dir: &Path) -> Result<()> {
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

            if commit_count >= 1000 && i % 2000 == 0 {
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn resolve_config_filename(config: &ConfigDef) -> String {
    config
        .filename
        .clone()
        .unwrap_or_else(|| match config.format.as_str() {
            "toml" => ".ferrflow.toml".to_string(),
            "json5" => "ferrflow.json5".to_string(),
            _ => "ferrflow.json".to_string(),
        })
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

#[derive(serde::Serialize)]
struct SerializableExpect<'a> {
    description: &'a str,
    check_contains: &'a [String],
    check_not_contains: &'a [String],
    output_order: &'a [String],
    packages_released: Option<usize>,
}
