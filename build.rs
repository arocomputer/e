//! Stamp release builds once; ordinary Cargo builds remain local and never self-update.
fn main() {
    for key in ["E_BUILD_VERSION", "E_BUILD_COMMIT", "E_BUILD_CHANNEL"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let version =
        std::env::var("E_BUILD_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned());
    let channel = std::env::var("E_BUILD_CHANNEL").unwrap_or_else(|_| "local".into());
    assert!(["local", "stable", "dev", "beta", "pr"].contains(&channel.as_str()));
    if channel == "stable" {
        assert_eq!(
            version,
            env!("CARGO_PKG_VERSION"),
            "stable version must match Cargo.toml"
        );
    } else if channel != "local" {
        assert!(
            version.starts_with(&format!("{}-{channel}.", env!("CARGO_PKG_VERSION"))),
            "preview identity must match manifest and channel"
        );
    }
    let commit = std::env::var("E_BUILD_COMMIT").unwrap_or_else(|_| "local".into());
    assert!(version
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)));
    assert!(
        commit == "local" || (commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_hexdigit()))
    );
    println!("cargo:rustc-env=E_VERSION={version}");
    println!("cargo:rustc-env=E_CHANNEL={channel}");
    println!("cargo:rustc-env=E_COMMIT={commit}");
}
