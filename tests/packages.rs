//! Packages: a git repository (or local directory) shaped like `~/.e/` —
//! `extensions/`, `skills/`, `prompts/`, `themes/` — installs under
//! `~/.e/packages/<host>/<path>`, is recorded in settings, and feeds every
//! loader after the home's own resources. Settings are the source of truth:
//! a deleted clone is reported at startup and restored by `e install`.
//!
//! These tests drive real `git` against a throwaway repository, the way a
//! user's install does.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{env_lock, serve_raw_bytes, Home};
use e::core::resources::packages::{self, Source, Status};

/// Package installs are async (release downloads); the git paths are
/// synchronous underneath, so a current-thread runtime is enough.
fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// A committed package repository with one resource of every kind, tagged
/// `v1`, plus a second commit adding `prompts/more.md` on `main`.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "e-pkg-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write(
            &dir.join("skills/hello/SKILL.md"),
            "---\nname: hello\ndescription: from a package\n---\nbody\n",
        );
        write(
            &dir.join("prompts/hi.md"),
            "---\ndescription: say hi\n---\nhi $1\n",
        );
        write(
            &dir.join("themes/pkgtheme.json"),
            r#"{"name":"pkgtheme","vars":{},"colors":{}}"#,
        );
        write(
            &dir.join("extensions/pkgext.sh"),
            "#!/bin/sh\nwhile IFS= read -r line; do\n\
             id=$(printf '%s' \"$line\" | sed -n 's/.*\"id\":\\([0-9][0-9]*\\).*/\\1/p')\n\
             case \"$line\" in\n\
             *initialize*) printf '{\"id\":%s,\"result\":{\"name\":\"pkgext\",\"version\":\"1\",\"commands\":[{\"name\":\"pkg\",\"description\":\"from pkg\"}]}}\\n' \"$id\" ;;\n\
             *shutdown*) exit 0 ;;\n\
             esac\ndone\n",
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.join("extensions/pkgext.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let repo = Repo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("one of each");
        repo.git(&["tag", "v1"]);
        write(&repo.dir.join("prompts/more.md"), "more\n");
        repo.commit("more");
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args)
            .current_dir(&self.dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// The `git:file://…` source for this repository, optionally pinned.
    fn source(&self, rev: Option<&str>) -> String {
        let base = format!("git:file://{}", self.dir.display());
        match rev {
            Some(rev) => format!("{base}@{rev}"),
            None => base,
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn settings_packages(home: &Home) -> Vec<String> {
    let text = std::fs::read_to_string(home.dir.join("settings.json")).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    json["packages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn install_clones_under_the_managed_root_and_every_loader_sees_the_package() {
    let _lock = env_lock();
    let home = Home::new("pkg-install");
    let repo = Repo::new("install");
    let cwd = std::env::temp_dir();

    let (root, counts) = block(packages::install(&repo.source(Some("v1")))).unwrap();
    assert!(root.starts_with(home.dir.join("packages").join("file")));
    assert_eq!(counts, [1, 1, 1, 1]);
    assert_eq!(settings_packages(&home), vec![repo.source(Some("v1"))]);
    assert!(
        !root.join("prompts/more.md").exists(),
        "pinned at v1, before the second commit"
    );

    let skills = e::core::resources::skills::list(&cwd);
    let hello = skills.iter().find(|s| s.name == "hello").unwrap();
    assert_eq!(hello.description, "from a package");
    assert!(hello.dir.starts_with(&root));
    assert!(packages::is_packaged(&hello.dir));

    let hi = e::core::resources::prompts::find("hi", &cwd).unwrap();
    assert_eq!(hi.content, "hi $1");

    assert!(e::core::config::settings::theme_names().contains(&"pkgtheme".to_string()));
    assert!(e::tui::theme::load_user("pkgtheme").is_some());

    let (notices, _rx) = tokio::sync::mpsc::channel(16);
    let host = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(e::core::extensions::ExtensionHost::start(notices, None));
    assert!(
        host.has_command("pkg"),
        "the package's extension is launched"
    );
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(host.shutdown());
}

#[test]
fn the_home_shadows_a_package_resource_of_the_same_name() {
    let _lock = env_lock();
    let home = Home::new("pkg-shadow");
    let repo = Repo::new("shadow");
    let cwd = std::env::temp_dir();
    block(packages::install(&repo.source(Some("v1")))).unwrap();
    write(
        &home.dir.join("skills/hello/SKILL.md"),
        "---\nname: hello\ndescription: from the home\n---\nbody\n",
    );
    write(&home.dir.join("prompts/hi.md"), "home hi\n");

    let skills = e::core::resources::skills::list(&cwd);
    let hellos: Vec<_> = skills.iter().filter(|s| s.name == "hello").collect();
    assert_eq!(hellos.len(), 1);
    assert_eq!(hellos[0].description, "from the home");
    assert_eq!(
        e::core::resources::prompts::find("hi", &cwd)
            .unwrap()
            .content,
        "home hi"
    );
}

#[test]
fn a_missing_clone_is_reported_at_startup_and_restored_by_install_all() {
    let _lock = env_lock();
    let home = Home::new("pkg-missing");
    let repo = Repo::new("missing");
    let (root, _) = block(packages::install(&repo.source(Some("v1")))).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(packages::missing(), vec![repo.source(Some("v1"))]);
    assert!(
        packages::dirs("skills").is_empty(),
        "nothing loads from a missing clone"
    );
    let (notices, mut rx) = tokio::sync::mpsc::channel(16);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let host = runtime.block_on(e::core::extensions::ExtensionHost::start(notices, None));
    let notice = rx.try_recv().unwrap();
    assert!(
        notice.starts_with("package git:file://")
            && notice.ends_with("not installed — run `e install`"),
        "{notice}"
    );
    runtime.block_on(host.shutdown());

    // Startup never cloned; `e install` with no source does.
    assert!(!root.exists());
    let results = block(packages::install_all());
    assert_eq!(results.len(), 1);
    assert!(results[0].as_ref().unwrap().ends_with(": installed"));
    assert!(root.join("skills/hello/SKILL.md").is_file());
    assert!(matches!(
        packages::list()[0].status,
        Status::Installed { .. }
    ));
    drop(home);
}

#[test]
fn reinstalling_moves_the_pin_and_unpinning_follows_the_default_branch() {
    let _lock = env_lock();
    let home = Home::new("pkg-pin");
    let repo = Repo::new("pin");
    let (root, _) = block(packages::install(&repo.source(Some("v1")))).unwrap();
    assert!(!root.join("prompts/more.md").exists());

    // Same package, new ref: one entry, moved — not a duplicate.
    let (same_root, counts) = block(packages::install(&repo.source(None))).unwrap();
    assert_eq!(same_root, root);
    assert_eq!(settings_packages(&home), vec![repo.source(None)]);
    assert_eq!(counts[2], 2, "the tip of main carries both prompts");

    // Unpinned, `e install` fast-forwards to new commits.
    write(&repo.dir.join("prompts/third.md"), "third\n");
    repo.commit("third");
    let results = block(packages::install_all());
    assert!(results[0].as_ref().unwrap().ends_with(": up to date"));
    assert!(root.join("prompts/third.md").is_file());
}

#[test]
fn remove_deletes_a_managed_clone_but_leaves_a_local_directory_alone() {
    let _lock = env_lock();
    let home = Home::new("pkg-remove");
    let repo = Repo::new("remove");
    let (root, _) = block(packages::install(&repo.source(Some("v1")))).unwrap();
    // Identity ignores the ref: removing by the bare source finds the entry.
    packages::remove(&repo.source(None)).unwrap();
    assert!(!root.exists());
    assert!(settings_packages(&home).is_empty());
    assert!(
        std::fs::read_dir(home.dir.join("packages"))
            .unwrap()
            .next()
            .is_none(),
        "empty host and user directories are pruned"
    );
    assert!(
        packages::remove(&repo.source(None)).is_err(),
        "not installed twice"
    );

    // A local path is referenced in place, so removal only forgets it.
    let local = repo.dir.to_string_lossy().into_owned();
    let (root, counts) = block(packages::install(&local)).unwrap();
    assert_eq!(root, repo.dir);
    assert_eq!(counts, [1, 1, 2, 1]);
    packages::remove(&local).unwrap();
    assert!(repo.dir.join("skills/hello/SKILL.md").is_file());
    assert!(settings_packages(&home).is_empty());
}

#[test]
fn a_source_that_is_not_a_package_is_refused_before_anything_is_written() {
    let _lock = env_lock();
    let home = Home::new("pkg-refuse");
    assert!(block(packages::install("intuitums/e-diff")).is_err());
    assert!(block(packages::install("--upload-pack=touch")).is_err());
    assert!(block(packages::install("/definitely/not/a/directory")).is_err());
    assert!(!home.dir.join("settings.json").exists());
    assert!(!home.dir.join("packages").exists());
}

/// A release asset served locally: the gzip tarball the workflow publishes
/// plus a matching checksums.txt.
fn release_server(name: &str) -> (u16, std::thread::JoinHandle<Vec<String>>, String) {
    let target = e::core::update::target().expect("a release target for this machine");
    let dir = std::env::temp_dir().join(format!("e-release-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(name),
        "#!/bin/sh\nread -r line; printf '{\"id\":1000000,\"result\":{\"name\":\"tool\"}}\n'\n",
    )
    .unwrap();
    let status = std::process::Command::new("tar")
        .args(["czf", "asset.tar.gz", name])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(status.success());
    let tar = std::fs::read(dir.join("asset.tar.gz")).unwrap();
    let sum = {
        use sha2::Digest;
        sha2::Sha256::digest(&tar)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let asset = format!("{name}-{target}.tar.gz");
    let http = |body: &[u8]| {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    };
    let sums = format!("{sum}  {asset}\n");
    let (port, server) = serve_raw_bytes(vec![http(&tar), http(sums.as_bytes())]);
    let _ = std::fs::remove_dir_all(&dir);
    (port, server, asset)
}

#[test]
fn a_release_package_installs_its_executable_under_extensions() {
    let _lock = env_lock();
    let home = Home::new("pkg-release");
    let (port, server, asset) = release_server("tool");
    let source = Source::parse("release:intuitums/e/tool@v9").unwrap();
    let base = format!("http://127.0.0.1:{port}");
    block(packages::install_release_from(&source, &base, &base)).unwrap();
    let requests = server.join().unwrap();
    assert!(requests[0].contains(&format!("GET /download/v9/{asset}")));
    assert!(requests[1].contains("GET /download/v9/checksums.txt"));

    let root = home.dir.join("packages/releases/intuitums/e/tool");
    assert_eq!(source.root(), root);
    let binary = root.join("extensions/tool");
    assert!(binary.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(&binary).unwrap().permissions().mode() & 0o111,
            0
        );
    }
    assert_eq!(
        e::core::update::installed_release_tag(&root).as_deref(),
        Some("v9")
    );
    assert!(!root.join(".staging-v9").exists(), "staging is cleaned up");
    // Pinned at the installed tag: a second install touches no network.
    block(packages::install_release_from(
        &source,
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
    ))
    .unwrap();

    // The parse grammar and identity.
    assert_eq!(
        Source::parse("release:Intuitums/E/tool@v9")
            .unwrap()
            .identity(),
        Source::parse("release:intuitums/e/tool")
            .unwrap()
            .identity()
    );
    assert!(Source::parse("release:intuitums/e").is_err());
    assert!(Source::parse("release:intuitums/../e/tool").is_err());
    assert!(Source::parse("release:intuitums/e/tool@-x").is_err());
}

#[test]
fn a_trusted_repository_lists_its_own_packages_and_once_roots_are_forgotten() {
    let _lock = env_lock();
    let home = Home::new("pkg-project");
    let repo = Repo::new("project");
    let ws = home.dir.join("ws");
    std::fs::create_dir_all(ws.join(".e")).unwrap();
    std::fs::write(
        ws.join(".e/packages"),
        format!(
            "# team packages\n{}\n\n{}\n",
            repo.source(Some("v1")),
            repo.dir.display()
        ),
    )
    .unwrap();
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&ws).unwrap();

    assert!(
        packages::configured().is_empty(),
        "untrusted: the list is ignored"
    );
    e::core::config::trust::set(&ws, true).unwrap();
    let listed = packages::configured();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(packages::missing(), vec![repo.source(Some("v1"))]);
    // `e install` installs the project's packages into the user's roots.
    let results = block(packages::install_all());
    assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
    assert_eq!(packages::roots().len(), 2);
    // Settings stay the user's: nothing from the project list was written.
    assert!(settings_packages(&home).is_empty());

    // `--package`: a local directory joins the roots in place; a clone is
    // temporary and removed by forget_once.
    let extra = home.dir.join("extra");
    std::fs::create_dir_all(extra.join("prompts")).unwrap();
    let root = block(packages::use_once(&extra.to_string_lossy())).unwrap();
    assert_eq!(root, extra);
    let cloned = block(packages::use_once(&repo.source(None))).unwrap();
    assert!(cloned.join("skills/hello/SKILL.md").is_file());
    assert_eq!(packages::roots().len(), 4);
    packages::forget_once();
    assert!(!cloned.exists(), "the temporary clone is gone");
    assert!(extra.is_dir(), "the local directory is not ours to delete");
    assert_eq!(packages::roots().len(), 2);
    std::env::set_current_dir(previous).unwrap();
}
