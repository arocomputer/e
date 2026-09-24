#!/usr/bin/env python3
"""Generate npm packages and a Homebrew formula from checksum-verified release archives."""

import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import tarfile

PLATFORMS = {
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "linux-arm64": "aarch64-unknown-linux-gnu",
    "linux-x64": "x86_64-unknown-linux-gnu",
}
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/release"))
from identity import identity



def prepare(tag, assets, output):
    """Require four verified binaries; derive every package version from the release tag."""
    release = identity(tag)
    version, channel, command = (release[key] for key in ("version", "channel", "command"))
    if version == "0.0.1":
        raise ValueError("v0.0.1 predates package-manager update protection")
    if channel != "production":
        raise ValueError("Only production releases are published as packages")
    checksums = {}
    for line in (assets / "checksums.txt").read_text().splitlines():
        digest, filename = line.split()
        filename = filename.lstrip("*")
        if filename in checksums or not re.fullmatch(r"[a-f0-9]{64}", digest):
            raise ValueError("Invalid or duplicate checksum")
        checksums[filename] = digest
    for target in PLATFORMS.values():
        filename = f"ulo-{target}.tar.gz"
        actual = hashlib.sha256((assets / filename).read_bytes()).hexdigest()
        if actual != checksums.get(filename):
            raise ValueError(f"Checksum mismatch: {filename}")
    output.mkdir(parents=True, exist_ok=False)
    common = {
        "version": version,
        "license": "MIT",
        "homepage": "https://ulo.sh",
        "repository": {"type": "git", "url": "git+https://github.com/arocomputer/ulo.git"},
        "publishConfig": {"access": "public", "tag": release["npm_tag"]},
    }
    for platform, target in PLATFORMS.items():
        folder = output / platform
        (folder / "bin").mkdir(parents=True)
        with tarfile.open(assets / f"ulo-{target}.tar.gz") as archive:
            member = archive.getmember("ulo")
            if not member.isfile():
                raise ValueError("Release executable must be a regular file")
            with (
                archive.extractfile(member) as source,
                (folder / "bin/ulo").open("wb") as dest,
            ):
                shutil.copyfileobj(source, dest)
        (folder / "bin/ulo").chmod(0o755)
        (folder / "bin/.ulo-install-method").write_text("npm\n")
        os_name, cpu = platform.split("-")
        manifest = dict(
            common,
            name=f"@arocomputer/ulo-{platform}",
            description=f"ulo binary for {platform}",
            os=[os_name],
            cpu=[cpu],
            files=["bin"],
        )
        if os_name == "linux":
            manifest["libc"] = ["glibc"]
        (folder / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
        shutil.copyfile(ROOT / "LICENSE", folder / "LICENSE")
    folder = output / "ulo"
    (folder / "bin").mkdir(parents=True)
    shutil.copy2(ROOT / "scripts/packaging/npm-launcher", folder / "bin/ulo")
    manifest = dict(
        common,
        name="@arocomputer/ulo",
        description="A small, extensible coding agent for your terminal",
        bin={command: "bin/ulo"},
        files=["bin"],
        optionalDependencies={
            f"@arocomputer/ulo-{platform}": version for platform in PLATFORMS
        },
    )
    (folder / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    shutil.copyfile(ROOT / "LICENSE", folder / "LICENSE")
    (folder / "README.md").write_text(
        f"# ulo\n\nInstall with `npm install -g @arocomputer/ulo@{release['npm_tag']}` or "
        f"`bun add -g @arocomputer/ulo@{release['npm_tag']}`.\n\nRun `{command}` to start. "
        "See https://ulo.sh/docs for setup.\n\n"
        "Includes native binaries for macOS and glibc Linux on ARM64 and x86-64.\n"
        "No install scripts or JavaScript runtime are needed to run the binary.\n"
    )
    # Slack versions its own content and publishes only with production releases.
    # Preview application channels must not change the bot's immutable package.
    source = ROOT / "services/slack"
    folder = output / "slack"
    folder.mkdir()
    for kind in ("bin", "src"):
        (folder / kind).mkdir()
        for path in sorted((source / kind).iterdir()):
            if not path.name.endswith(".test.ts"):
                shutil.copy2(path, folder / kind / path.name)
    for name in (".env.example", "manifest.json", "README.md"):
        shutil.copy2(source / name, folder / name)
    shutil.copyfile(ROOT / "LICENSE", folder / "LICENSE")
    manifest = json.loads((source / "package.json").read_text())
    manifest["publishConfig"] = {"access": "public"}
    # Only the bot and its metadata are published: no lifecycle scripts and no
    # development dependencies, nothing the consumer did not ask for.
    manifest.pop("devDependencies", None)
    manifest.pop("scripts", None)
    (folder / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    formula = [
        "class Ulo < Formula",
        '  desc "Small, extensible coding agent for your terminal"',
        '  homepage "https://ulo.sh"',
        f'  version "{version}"',
        '  license "MIT"',
        "",
    ]
    for os_name, ruby_os in [("darwin", "macos"), ("linux", "linux")]:
        formula.append(f"  on_{ruby_os} do")
        for cpu, ruby_cpu in [("arm64", "arm"), ("x64", "intel")]:
            target = PLATFORMS[f"{os_name}-{cpu}"]
            filename = f"ulo-{target}.tar.gz"
            formula.extend(
                [
                    f"    on_{ruby_cpu} do",
                    f'      url "https://github.com/{release["repository"]}/releases/download/{tag}/{filename}"',
                    f'      sha256 "{checksums[filename]}"',
                    "    end",
                ]
            )
        formula.extend(["  end", ""])
    formula.extend(
        [
            "  def install",
            '    libexec.install "ulo"',
            f'    (libexec/".ulo-install-method").write "homebrew\\n"',
            f'    bin.install_symlink libexec/"ulo" => "{command}"',
            "  end",
            "",
            "  test do",
            f'    assert_equal "ulo #{{version}}", shell_output("#{{bin}}/{command} --version").strip',
            "  end",
            "end",
            "",
        ]
    )
    (output / f"{command}.rb").write_text("\n".join(formula))


if __name__ == "__main__":
    prepare(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
