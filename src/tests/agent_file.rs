use super::*;

#[test]
fn agent_file_path_allows_top_level_md_and_docs_parts() {
    assert_eq!(
        resolve_agent_editable_rel_path("manual-check.md").unwrap(),
        "manual-check.md"
    );
    assert_eq!(
        resolve_agent_editable_rel_path("./notes.md").unwrap(),
        "notes.md"
    );
    assert_eq!(
        resolve_agent_editable_rel_path("docs/design/api.md").unwrap(),
        "docs/design/api.md"
    );
}

#[test]
fn agent_file_path_rejects_traversal_and_non_md() {
    assert!(resolve_agent_editable_rel_path("../escape.md").is_err());
    assert!(resolve_agent_editable_rel_path("/abs/path.md").is_err());
    assert!(resolve_agent_editable_rel_path("docs/../../x.md").is_err());
    assert!(resolve_agent_editable_rel_path("state.json").is_err());
    assert!(resolve_agent_editable_rel_path("").is_err());
    assert!(resolve_agent_editable_rel_path("a/b/c.md").is_err());
    assert!(resolve_agent_editable_rel_path("docs/x.md").is_err());
    assert!(resolve_agent_editable_rel_path("sub/file.md").is_err());
    assert!(resolve_agent_editable_rel_path(".md").is_err());
}

#[test]
fn agent_file_path_rejects_meta_md_but_allows_other_top_files() {
    assert!(resolve_agent_editable_rel_path("meta.md").is_err());
    assert!(resolve_agent_editable_rel_path("test.md").is_ok());
    assert!(resolve_agent_editable_rel_path("manual-check.md").is_ok());
}

#[test]
fn agent_file_sha1_hex_is_stable() {
    assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(sha1_hex(""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
}
