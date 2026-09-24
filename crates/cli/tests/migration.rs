//! The rename preserves an existing home and lets the new directory take precedence.
mod common;
use common::{env_lock, Home};
use std::path::Path;

/// Ask the real CLI to resolve its home without inheriting the fixture's override.
fn reported_home(base: &Path, override_home: Option<&Path>) -> String {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ulo"));
    command
        .args(["doctor", "--json", "--no-network"])
        .env("HOME", base)
        .env_remove("ULO_HOME");
    if let Some(path) = override_home {
        command.env("ULO_HOME", path);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    report["ulo_home"].as_str().unwrap().to_owned()
}

#[test]
fn existing_home_survives_the_rename_and_explicit_home_wins() {
    let _guard = env_lock();
    let fixture = Home::new("rename-home");
    let suffix = match ulo::core::CHANNEL {
        "local" => "-dev",
        "pr" => "-pr",
        _ => "",
    };
    let legacy = fixture.dir.join(format!(".e{suffix}"));
    let current = fixture.dir.join(format!(".ulo{suffix}"));
    std::fs::create_dir_all(&legacy).unwrap();
    let active = |path: &Path| {
        if ulo::core::CHANNEL == "pr" {
            path.join(ulo::core::COMMIT)
        } else {
            path.to_owned()
        }
    };
    assert_eq!(
        reported_home(&fixture.dir, None),
        active(&legacy).to_string_lossy()
    );
    std::fs::create_dir_all(&current).unwrap();
    assert_eq!(
        reported_home(&fixture.dir, None),
        active(&current).to_string_lossy()
    );
    assert_eq!(
        reported_home(&fixture.dir, Some(&fixture.dir)),
        fixture.dir.to_string_lossy()
    );
    assert!(
        legacy.is_dir(),
        "resolving a home must not move or delete user state"
    );
}

#[test]
fn workspace_resources_prefer_the_new_directory_without_merging_stores() {
    let _guard = env_lock();
    let fixture = Home::new("rename-workspace");
    let legacy = fixture.dir.join(".e");
    let current = fixture.dir.join(".ulo");
    std::fs::create_dir_all(&legacy).unwrap();
    assert_eq!(
        ulo::core::config::home::workspace_directory(&fixture.dir),
        legacy
    );
    std::fs::create_dir_all(&current).unwrap();
    assert_eq!(
        ulo::core::config::home::workspace_directory(&fixture.dir),
        current
    );
}
