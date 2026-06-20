//! Pure folder-tree helpers over `bru_core` collection structures. No gpui.

use bru_core::Folder;
use std::path::{Path, PathBuf};
/// Flatten every request in a folder tree into `(name, path)` (recursive).
pub fn flatten_requests(folder: &Folder, out: &mut Vec<(String, PathBuf)>) {
    for sub in &folder.folders {
        flatten_requests(sub, out);
    }
    for req in &folder.requests {
        out.push((req.name.clone(), req.path.clone()));
    }
}
/// Whether a folder (or any descendant request/folder name) matches `query`.
pub fn folder_matches(folder: &Folder, query: &str) -> bool {
    folder.name.to_lowercase().contains(query)
        || folder
            .requests
            .iter()
            .any(|r| r.name.to_lowercase().contains(query))
        || folder.folders.iter().any(|f| folder_matches(f, query))
}
/// Find the sub-folder whose path is `dir`.
pub fn find_folder<'a>(folder: &'a Folder, dir: &Path) -> Option<&'a Folder> {
    for sub in &folder.folders {
        if sub.path == dir {
            return Some(sub);
        }
        if let Some(f) = find_folder(sub, dir) {
            return Some(f);
        }
    }
    None
}

/// Collect every request path under `folder` (recursive).
pub fn collect_folder_requests(folder: &Folder, out: &mut Vec<PathBuf>) {
    for sub in &folder.folders {
        collect_folder_requests(sub, out);
    }
    for req in &folder.requests {
        out.push(req.path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bru_core::RequestItem;

    fn req(name: &str) -> RequestItem {
        RequestItem {
            name: name.to_string(),
            path: PathBuf::from(format!("{name}.bru")),
            method: Some("GET".to_string()),
            seq: Some(1),
        }
    }

    /// root { requests: [top], folders: [sub { requests: [deep] }] }
    fn tree() -> Folder {
        Folder {
            name: "root".to_string(),
            path: PathBuf::from("root"),
            folders: vec![Folder {
                name: "sub".to_string(),
                path: PathBuf::from("root/sub"),
                folders: vec![],
                requests: vec![req("deep")],
            }],
            requests: vec![req("top")],
        }
    }

    #[test]
    fn flatten_requests_children_first() {
        let mut out = Vec::new();
        flatten_requests(&tree(), &mut out);
        let names: Vec<_> = out.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["deep", "top"]);
    }

    #[test]
    fn collect_folder_requests_gathers_all_paths() {
        let mut out = Vec::new();
        collect_folder_requests(&tree(), &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|p| p.ends_with("deep.bru")));
        assert!(out.iter().any(|p| p.ends_with("top.bru")));
    }

    #[test]
    fn find_folder_locates_by_path() {
        let t = tree();
        let found = find_folder(&t, &PathBuf::from("root/sub"));
        assert_eq!(found.map(|f| f.name.as_str()), Some("sub"));
        assert!(find_folder(&t, &PathBuf::from("root/missing")).is_none());
    }

    #[test]
    fn folder_matches_name_or_descendant() {
        let t = tree();
        assert!(folder_matches(&t, "sub")); // sub-folder name
        assert!(folder_matches(&t, "deep")); // descendant request
        assert!(folder_matches(&t, "top")); // own request
        assert!(!folder_matches(&t, "zzz"));
    }
}
