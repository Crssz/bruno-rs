//! Collection-folder loader test against a self-contained sample collection.

use std::path::PathBuf;

fn sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/sample")
}

#[test]
fn loads_tree_with_names_methods_seq_and_subfolder() {
    let tree = bru_lang::load_collection(&sample_dir()).expect("load sample collection");

    // Name comes from bruno.json.
    assert_eq!(tree.name, "Sample API");

    // Root requests sorted by seq; collection.bru is excluded.
    let names: Vec<&str> = tree.root.requests.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Get Users", "Create User"]);
    assert_eq!(tree.root.requests[0].method.as_deref(), Some("GET"));
    assert_eq!(tree.root.requests[1].method.as_deref(), Some("POST"));
    assert_eq!(tree.root.requests[0].seq, Some(1));

    // One subfolder, named from its folder.bru, with its own request.
    assert_eq!(tree.root.folders.len(), 1);
    let admin = &tree.root.folders[0];
    assert_eq!(admin.name, "Admin");
    let admin_reqs: Vec<&str> = admin.requests.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(admin_reqs, ["Delete User"]);
    assert_eq!(admin.requests[0].method.as_deref(), Some("DELETE"));
}
