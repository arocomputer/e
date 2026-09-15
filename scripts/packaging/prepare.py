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


def prepare(tag, assets, output):
    """Require four verified binaries; derive every package version from the release tag."""
    if not re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", tag):
        raise ValueError("Expected a stable vX.Y.Z release tag")
    version = tag[1:]
    if tuple(map(int, version.split("."))) < (0, 0, 2):
        raise ValueError(
            "v0.0.1 predates package-manager update protection; publish a newer release"
        )
    checksums = {}
    for line in (assets / "checksums.txt").read_text().splitlines():
        digest, filename = line.split()
        filename = filename.lstrip("*")
        if filename in checksums or not re.fullmatch(r"[a-f0-9]{64}", digest):
            raise ValueError("Invalid or duplicate checksum")
        checksums[filename] = digest
    for target in PLATFORMS.values():
        filename = f"e-{target}.tar.gz"
        actual = hashlib.sha256((assets / filename).read_bytes()).hexdigest()
        if actual != checksums.get(filename):
            raise ValueError(f"Checksum mismatch: {filename}")
    output.mkdir(parents=True, exist_ok=False)
    common = {
        "version": version,
        "license": "MIT",
        "homepage": "https://e.intuitum.sh",
        "repository": {"type": "git", "url": "git+https://github.com/intuitums/e.git"},
        "publishConfig": {"access": "public"},
    }
    for platform, target in PLATFORMS.items():
        folder = output / platform
        (folder / "bin").mkdir(parents=True)
        with tarfile.open(assets / f"e-{target}.tar.gz") as archive:
            member = archive.getmember("e")
            if not member.isfile():
                raise ValueError("Release executable must be a regular file")
            with (
                archive.extractfile(member) as source,
                (folder / "bin/e").open("wb") as dest,
            ):
                shutil.copyfileobj(source, dest)
        (folder / "bin/e").chmod(0o755)
        (folder / "bin/.e-install-method").write_text("npm\n")
        os_name, cpu = platform.split("-")
        manifest = dict(
            common,
            name=f"@intuitums/e-{platform}",
            description=f"e binary for {platform}",
            os=[os_name],
            cpu=[cpu],
            files=["bin"],
        )
        if os_name == "linux":
            manifest["libc"] = ["glibc"]
        (folder / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
        shutil.copyfile(ROOT / "LICENSE", folder / "LICENSE")
    folder = output / "e"
    (folder / "bin").mkdir(parents=True)
    shutil.copy2(ROOT / "packaging/npm/e", folder / "bin/e")
    manifest = dict(
        common,
        name="@intuitums/e",
        description="A small, extensible coding agent for your terminal",
        bin={"e": "bin/e"},
        files=["bin"],
        optionalDependencies={
            f"@intuitums/e-{platform}": version for platform in PLATFORMS
        },
    )
    (folder / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    shutil.copyfile(ROOT / "LICENSE", folder / "LICENSE")
    (folder / "README.md").write_text(
        "# e\n\nInstall with `npm install -g @intuitums/e` or `bun add -g @intuitums/e`.\n\nRun `e` to start. See https://e.intuitum.sh/docs for setup.\n\nIncludes native binaries for macOS and glibc Linux on ARM64 and x86-64.\nNo install scripts or JavaScript runtime are needed to run the binary.\n"
    )
    formula = [
        "class E < Formula",
        '  desc "Small, extensible coding agent for your terminal"',
        '  homepage "https://e.intuitum.sh"',
        f'  version "{version}"',
        '  license "MIT"',
        "",
    ]
    for os_name, ruby_os in [("darwin", "macos"), ("linux", "linux")]:
        formula.append(f"  on_{ruby_os} do")
        for cpu, ruby_cpu in [("arm64", "arm"), ("x64", "intel")]:
            target = PLATFORMS[f"{os_name}-{cpu}"]
            filename = f"e-{target}.tar.gz"
            formula.extend(
                [
                    f"    on_{ruby_cpu} do",
                    f'      url "https://github.com/intuitums/e/releases/download/{tag}/{filename}"',
                    f'      sha256 "{checksums[filename]}"',
                    "    end",
                ]
            )
        formula.extend(["  end", ""])
    formula.extend(
        [
            "  def install",
            '    libexec.install "e"',
            '    (libexec/".e-install-method").write "homebrew\\n"',
            '    bin.install_symlink libexec/"e"',
            "  end",
            "",
            "  test do",
            '    assert_equal "e #{version}", shell_output("#{bin}/e --version").strip',
            "  end",
            "end",
            "",
        ]
    )
    (output / "e.rb").write_text("\n".join(formula))


if __name__ == "__main__":
    prepare(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
