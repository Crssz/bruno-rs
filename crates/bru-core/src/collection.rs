//! On-disk collection tree types (pure data — no IO). The loader that walks a
//! folder and fills these lives in `bru-lang` (it needs the parser).

use std::path::PathBuf;

/// A loaded Bruno collection: a named root folder of requests and sub-folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionTree {
    pub name: String,
    pub root: Folder,
}

/// A folder node (the collection root is itself a `Folder`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
    pub folders: Vec<Folder>,
    pub requests: Vec<RequestItem>,
}

/// A single request `.bru` file in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestItem {
    /// `meta.name`, falling back to the file stem.
    pub name: String,
    pub path: PathBuf,
    /// Uppercase HTTP method for display, if the file has a method block.
    pub method: Option<String>,
    pub seq: Option<i64>,
}
