use anyhow::{bail, Result};
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

    pub fn insert_blob(&mut self, path: &str, blob_oid: Oid) -> Result<()> {
        self.invalidate();
        if let Some(slash) = path.find('/') {
            let dir = &path[..slash];
            let rest = &path[slash + 1..];
            let child = self
                .entries
                .entry(dir.to_string())
                .or_insert_with(|| TreeEntry::Tree(TreeNode::new()));
            match child {
                TreeEntry::Tree(node) => node.insert_blob(rest, blob_oid)?,
                _ => {
                    bail!("path conflict: '{dir}' is a blob, not a tree (while inserting '{path}')")
                }
            }
        } else {
            self.entries
                .insert(path.to_string(), TreeEntry::Blob(blob_oid));
        }
        Ok(())
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
        self.root.insert_blob(path, blob_oid)?;
        Ok(())
    }

    pub fn append_dummy(&mut self, repo: &Repository, path: &str) -> Result<()> {
        let content = self.dummy_content.entry(path.to_string()).or_default();
        content.extend_from_slice(b"change\n");
        let blob_oid = repo.blob(content)?;
        self.root.insert_blob(path, blob_oid)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo() -> (TempDir, Repository) {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    #[test]
    fn insert_blob_flat() {
        let (_tmp, repo) = init_repo();
        let blob = repo.blob(b"hello").unwrap();

        let mut root = TreeNode::new();
        root.insert_blob("file.txt", blob).unwrap();

        let tree_oid = root.write(&repo).unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        assert!(tree.get_name("file.txt").is_some());
    }

    #[test]
    fn insert_blob_nested() {
        let (_tmp, repo) = init_repo();
        let blob = repo.blob(b"data").unwrap();

        let mut root = TreeNode::new();
        root.insert_blob("a/b/c.txt", blob).unwrap();

        let tree_oid = root.write(&repo).unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();

        let a_entry = tree.get_name("a").unwrap();
        let a_tree = repo.find_tree(a_entry.id()).unwrap();
        let b_entry = a_tree.get_name("b").unwrap();
        let b_tree = repo.find_tree(b_entry.id()).unwrap();
        assert!(b_tree.get_name("c.txt").is_some());
    }

    #[test]
    fn insert_blob_overwrites_same_path() {
        let (_tmp, repo) = init_repo();
        let blob1 = repo.blob(b"v1").unwrap();
        let blob2 = repo.blob(b"v2").unwrap();

        let mut root = TreeNode::new();
        root.insert_blob("f.txt", blob1).unwrap();
        root.insert_blob("f.txt", blob2).unwrap();

        let tree_oid = root.write(&repo).unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let entry = tree.get_name("f.txt").unwrap();
        let obj = repo.find_blob(entry.id()).unwrap();
        assert_eq!(obj.content(), b"v2");
    }

    #[test]
    fn cached_oid_invalidated_on_insert() {
        let (_tmp, repo) = init_repo();
        let blob1 = repo.blob(b"first").unwrap();
        let blob2 = repo.blob(b"second").unwrap();

        let mut root = TreeNode::new();
        root.insert_blob("a.txt", blob1).unwrap();
        let oid1 = root.write(&repo).unwrap();

        root.insert_blob("b.txt", blob2).unwrap();
        let oid2 = root.write(&repo).unwrap();

        assert_ne!(oid1, oid2);
    }

    #[test]
    fn insert_blob_conflicts_with_existing_blob() {
        let (_tmp, repo) = init_repo();
        let blob = repo.blob(b"x").unwrap();

        let mut root = TreeNode::new();
        root.insert_blob("f", blob).unwrap();
        // Try to insert a nested path where "f" is already a blob
        let err = root.insert_blob("f/g.txt", blob).unwrap_err();
        assert!(err.to_string().contains("path conflict"));
    }

    #[test]
    fn builder_set_file_and_commit() {
        let (_tmp, repo) = init_repo();
        let mut builder = BulkRepoBuilder::new();
        let time = Time::new(1_700_000_000, 0);

        builder.set_file(&repo, "README.md", b"# hello").unwrap();
        let oid = builder
            .commit(&repo, None, "initial commit", &time)
            .unwrap();

        let commit = repo.find_commit(oid).unwrap();
        assert_eq!(commit.message(), Some("initial commit"));

        let tree = commit.tree().unwrap();
        assert!(tree.get_name("README.md").is_some());
    }

    #[test]
    fn builder_append_dummy_accumulates() {
        let (_tmp, repo) = init_repo();
        let mut builder = BulkRepoBuilder::new();
        let time = Time::new(1_700_000_000, 0);

        builder.append_dummy(&repo, "src/main.rs").unwrap();
        let c1 = builder.commit(&repo, None, "first", &time).unwrap();

        builder.append_dummy(&repo, "src/main.rs").unwrap();
        let c2 = builder.commit(&repo, Some(c1), "second", &time).unwrap();

        // The two commits should have different trees (file content changed)
        let t1 = repo.find_commit(c1).unwrap().tree_id();
        let t2 = repo.find_commit(c2).unwrap().tree_id();
        assert_ne!(t1, t2);
    }

    #[test]
    fn builder_commit_chain() {
        let (_tmp, repo) = init_repo();
        let mut builder = BulkRepoBuilder::new();
        let time = Time::new(1_700_000_000, 0);

        builder.set_file(&repo, "f.txt", b"a").unwrap();
        let c1 = builder.commit(&repo, None, "c1", &time).unwrap();

        builder.set_file(&repo, "f.txt", b"b").unwrap();
        let c2 = builder.commit(&repo, Some(c1), "c2", &time).unwrap();

        let commit2 = repo.find_commit(c2).unwrap();
        assert_eq!(commit2.parent_count(), 1);
        assert_eq!(commit2.parent_id(0).unwrap(), c1);
    }
}
