use anyhow::Result;
use git2::{FileMode, Oid, Repository, Signature, Time};
use std::collections::HashMap;

pub enum TreeEntry {
    Blob(Oid),
    Tree(TreeNode),
}

pub struct TreeNode {
    entries: HashMap<String, TreeEntry>,
    cached_oid: Option<Oid>,
}

impl TreeNode {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            cached_oid: None,
        }
    }

    fn invalidate(&mut self) {
        self.cached_oid = None;
    }

    pub fn insert_blob(&mut self, path: &str, blob_oid: Oid) {
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

    pub fn write(&mut self, repo: &Repository) -> Result<Oid> {
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

pub struct BulkRepoBuilder {
    root: TreeNode,
    dummy_content: HashMap<String, Vec<u8>>,
}

impl BulkRepoBuilder {
    pub fn new() -> Self {
        Self {
            root: TreeNode::new(),
            dummy_content: HashMap::new(),
        }
    }

    pub fn set_file(&mut self, repo: &Repository, path: &str, content: &[u8]) -> Result<()> {
        let blob_oid = repo.blob(content)?;
        self.root.insert_blob(path, blob_oid);
        Ok(())
    }

    pub fn append_dummy(&mut self, repo: &Repository, path: &str) -> Result<()> {
        let content = self.dummy_content.entry(path.to_string()).or_default();
        content.extend_from_slice(b"change\n");
        let blob_oid = repo.blob(content)?;
        self.root.insert_blob(path, blob_oid);
        Ok(())
    }

    pub fn commit(
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
