use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FixtureDef {
    pub meta: Meta,
    #[serde(default)]
    pub config: Option<ConfigDef>,
    #[serde(default)]
    pub packages: Vec<PackageDef>,
    #[serde(default)]
    pub commits: Vec<CommitDef>,
    #[serde(default)]
    pub tags: Vec<TagDef>,
    #[serde(default)]
    pub branches: Vec<BranchDef>,
    #[serde(default)]
    pub hooks: Vec<HookFileDef>,
    #[serde(default)]
    pub generate: Option<GenerateDef>,
    #[serde(default)]
    pub expect: Option<ExpectDef>,
}

#[derive(Debug, Deserialize)]
pub struct Meta {
    #[allow(dead_code)]
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigDef {
    pub content: String,
    #[serde(default = "default_config_format")]
    pub format: String,
    #[serde(default)]
    pub filename: Option<String>,
}

fn default_config_format() -> String {
    "json".to_string()
}

#[derive(Debug, Deserialize)]
pub struct PackageDef {
    pub name: String,
    pub path: String,
    pub initial_version: String,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommitDef {
    pub message: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub merge: bool,
}

#[derive(Debug, Deserialize)]
pub struct TagDef {
    pub name: String,
    pub at_commit: i32,
}

#[derive(Debug, Deserialize)]
pub struct BranchDef {
    pub name: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub at_commit: Option<i32>,
    #[serde(default)]
    pub commits: Vec<CommitDef>,
    #[serde(default)]
    pub merge: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HookFileDef {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct GenerateDef {
    #[serde(default = "default_gen_packages")]
    pub packages: usize,
    #[serde(default = "default_gen_commits")]
    pub commits: usize,
    #[serde(default = "default_gen_seed")]
    pub seed: u64,
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
pub struct ExpectDef {
    #[serde(default)]
    pub check_contains: Vec<String>,
    #[serde(default)]
    pub check_not_contains: Vec<String>,
    #[serde(default)]
    pub output_order: Vec<String>,
    #[serde(default)]
    pub packages_released: Option<usize>,
}

#[derive(serde::Serialize)]
pub struct SerializableExpect<'a> {
    pub description: &'a str,
    pub check_contains: &'a [String],
    pub check_not_contains: &'a [String],
    pub output_order: &'a [String],
    pub packages_released: Option<usize>,
}

pub fn resolve_config_filename(config: &ConfigDef) -> String {
    config
        .filename
        .clone()
        .unwrap_or_else(|| match config.format.as_str() {
            "toml" => ".ferrflow.toml".to_string(),
            "json5" => "ferrflow.json5".to_string(),
            _ => "ferrflow.json".to_string(),
        })
}
