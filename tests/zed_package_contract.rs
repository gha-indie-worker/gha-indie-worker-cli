#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

use toml::Value;

fn repo_file(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn parse_toml(path: &str) -> Value {
    let text = fs::read_to_string(repo_file(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    toml::from_str(&text).unwrap_or_else(|error| panic!("cannot parse {path}: {error}"))
}

fn required_str<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    path.iter()
        .fold(value, |current, key| {
            current
                .get(*key)
                .unwrap_or_else(|| panic!("missing TOML key {}", path.join(".")))
        })
        .as_str()
        .unwrap_or_else(|| panic!("TOML key {} is not a string", path.join(".")))
}

#[test]
fn zed_package_metadata_matches_the_rust_binary_contract() {
    let package = parse_toml(".zpkg.toml");
    let cargo = parse_toml("Cargo.toml");
    let lock = parse_toml(".zpkg.lock");

    let cargo_package_name = required_str(&cargo, &["package", "name"]);
    let cargo_bin = cargo
        .get("bin")
        .and_then(Value::as_array)
        .and_then(|bins| bins.first())
        .expect("Cargo.toml must declare one [[bin]] entry");
    let cargo_bin_name = required_str(cargo_bin, &["name"]);
    let cargo_bin_path = required_str(cargo_bin, &["path"]);

    assert_eq!(required_str(&package, &["package", "org"]), "gha-indie-worker");
    assert_eq!(
        required_str(&package, &["package", "name"]),
        cargo_package_name,
        ".zpkg.toml package.name must track Cargo.toml package.name"
    );
    assert_eq!(
        required_str(&package, &["package", "version"]),
        required_str(&cargo, &["package", "version"]),
        ".zpkg.toml package.version must track Cargo.toml package.version"
    );
    assert_eq!(cargo_bin_path, "src/main.rs");

    let build_command = required_str(&package, &["build", "command"]);
    assert!(
        build_command.split_whitespace().any(|arg| arg == "--locked"),
        "Zed build must consume the committed Cargo.lock"
    );
    assert!(
        build_command.contains(&format!("--bin {cargo_bin_name}")),
        "Zed build must target the Cargo binary"
    );

    let expected_binary = format!("target/release/{cargo_bin_name}");
    assert_eq!(
        required_str(&package, &["bin", cargo_bin_name]),
        expected_binary
    );
    assert!(
        required_str(&package, &["scripts", "test"]).contains("--locked"),
        "Zed package test command must consume the committed Cargo.lock"
    );
    assert_eq!(
        lock.get("version").and_then(Value::as_integer),
        Some(1),
        ".zpkg.lock must use the supported lock format"
    );
}
