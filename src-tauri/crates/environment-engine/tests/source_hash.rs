use environment_engine::payload::build_payload;

#[test]
fn source_hash_matches_cli_vectors_before_install_filtering() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cli-content-hashes.json")).unwrap();
    for vector in fixture["vectors"].as_array().unwrap() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        for (relative, content) in vector["files"].as_object().unwrap() {
            let path = relative
                .split('/')
                .fold(source.clone(), |path, part| path.join(part));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content.as_str().unwrap()).unwrap();
        }
        let built = build_payload(&source, &temp.path().join("payload")).unwrap();
        assert_eq!(
            built.computed_hash,
            vector["expected"].as_str().unwrap(),
            "{}",
            vector["name"]
        );
    }
}
