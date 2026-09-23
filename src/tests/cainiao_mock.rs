use super::*;

#[test]
fn tls_cert_paths_missing_when_dir_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(tls_cert_paths(dir.path()).is_none());
}

#[test]
fn tls_cert_paths_ready_when_cert_and_key_exist() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("localhost.crt"), "cert").unwrap();
    std::fs::write(dir.path().join("localhost.key"), "key").unwrap();
    let (ca, cert, key) = tls_cert_paths(dir.path()).expect("tls should be ready");
    assert_eq!(ca, dir.path().join("ca.crt"));
    assert_eq!(cert, dir.path().join("localhost.crt"));
    assert_eq!(key, dir.path().join("localhost.key"));
}

#[test]
fn tls_cert_paths_missing_when_only_cert() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("localhost.crt"), "cert").unwrap();
    assert!(tls_cert_paths(dir.path()).is_none());
}
