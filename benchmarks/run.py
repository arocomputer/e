#!/usr/bin/env python3
"""ulo's benchmark suite: the numbers ulo's identity depends on.

Measures the release binary — build it first (`cargo build --release`) or let
this script do it. Normal runs write a timestamped report. `--check` applies
deliberately generous cross-runner budgets and writes nothing, making it a
stable regression alarm rather than a microbenchmark contest.
"""
import argparse, datetime, hashlib, fcntl, json, os, platform, pty, re, select, shutil
import statistics, struct, subprocess, sys, termios, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BINARY = os.path.join(ROOT, "target", "release", "ulo")
BUDGETS = os.path.join(ROOT, "benchmarks", "budgets.json")

# A real extension: answers the initialize handshake, then idles until ulo
# closes stdin. No hooks declared, so hook.startup is never sent to it.
EXTENSION_SH = r"""#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{"id":%s,"result":{"name":"bench-ext","version":"0.0.0"}}\n' "$id"
      ;;
  esac
done
"""


def build():
    subprocess.run(["cargo", "build", "--release", "--locked"], cwd=ROOT, check=True,
                   capture_output=True)


def binary_size():
    return os.path.getsize(BINARY)


def long_session_frame():
    """Measure cached transcript assembly and dock painting into a sink."""
    result = subprocess.run(
        ["cargo", "test", "--release", "--locked", "--lib",
         "long_session_frame_benchmark", "--", "--ignored", "--nocapture"],
        cwd=ROOT, capture_output=True, text=True, check=True)
    match = re.search(r"renderer 10000 blocks: ([0-9.]+) ms/frame", result.stderr)
    if not match:
        raise RuntimeError("renderer benchmark did not report its measurement")
    return float(match.group(1))


def cold_start_version(runs=20):
    """Process spawn to exit for `ulo --version`: the floor of every launch."""
    samples = []
    for _ in range(runs):
        t0 = time.perf_counter()
        subprocess.run([BINARY, "--version"], capture_output=True, check=True)
        samples.append((time.perf_counter() - t0) * 1000)
    return statistics.median(samples)


def make_loaded_home():
    """Build a home shaped like a working one — the read set a real user's
    cold start touches: global and package skills, package prompts, two live
    extensions (one shipped in the package, one in the home), a full prompt
    history, and settings. Return the home's path."""
    root = f"/tmp/ulo-bench-loaded-{os.getpid()}"
    shutil.rmtree(root, ignore_errors=True)
    os.makedirs(root)
    package = os.path.join(root, "packages", "bench-pkg")
    for count, skills_dir in [(24, os.path.join(root, "skills")),
                              (6, os.path.join(package, "skills"))]:
        for i in range(count):
            skill = os.path.join(skills_dir, f"bench-skill-{i:02d}")
            os.makedirs(skill)
            with open(os.path.join(skill, "SKILL.md"), "w", encoding="utf-8") as file:
                file.write(
                    "---\n"
                    f"description: Benchmark skill {i} — carried to give the "
                    "cold start a real catalog to read.\n"
                    "---\n"
                    "A small body, enough to make each skill a real file read.\n")
    prompts = os.path.join(package, "prompts")
    os.makedirs(prompts)
    for i in range(3):
        with open(os.path.join(prompts, f"bench-prompt-{i}.md"), "w",
                  encoding="utf-8") as file:
            file.write(f"Prompt template {i}: summarize the current session.\n")
    for extensions_dir in [os.path.join(root, "extensions"),
                           os.path.join(package, "extensions")]:
        os.makedirs(extensions_dir)
        script = os.path.join(extensions_dir, "bench-ext.sh")
        with open(script, "w", encoding="utf-8") as file:
            file.write(EXTENSION_SH)
        os.chmod(script, 0o755)
    with open(os.path.join(root, "history.jsonl"), "w", encoding="utf-8") as file:
        for i in range(1000):
            file.write(json.dumps(
                f"benchmark prompt {i}: fix the flaky test in module {i % 12}\n"))
    with open(os.path.join(root, "settings.json"), "w", encoding="utf-8") as file:
        json.dump({"theme": "dark", "packages": [package]}, file)
    return root


def evict_binary():
    """Best-effort drop of the binary's pages from the OS file cache:
    posix_fadvise on Linux, F_NOCACHE on macOS; elsewhere a no-op, where the
    cold-launch number reads as a single launch with a warm cache."""
    if hasattr(os, "posix_fadvise"):
        fd = os.open(BINARY, os.O_RDONLY)
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        os.close(fd)
    elif platform.system() == "Darwin":
        fd = os.open(BINARY, os.O_RDONLY)
        fcntl.fcntl(fd, 48, 1)  # F_NOCACHE
        while os.read(fd, 1 << 20):
            pass
        os.close(fd)


def boot_sample(home, marker=b"Run /help", argv=("ulo",)):
    """One pty spawn → marker on screen, measured in ms. The marker is the
    first thing that proves the wanted frame painted: the fresh-session
    banner by default, or a resumed session's tail. Answers the OSC 11
    background query the way a real terminal does — otherwise ulo's 400 ms
    detection timeout dominates the number. Reaps with WNOHANG: blocking
    waitpid on a SIGKILLed pty child can wedge on macOS."""
    t0 = time.perf_counter()
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(BINARY, list(argv), dict(os.environ, ULO_HOME=home, TERM="xterm-256color"))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    buf = b""
    answered = False
    deadline = time.time() + 10
    while marker not in buf and time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.02)
        if r:
            try:
                buf += os.read(fd, 65536)
            except OSError:
                break
        if not answered and b"\x1b]11;?" in buf:
            os.write(fd, b"\x1b]11;rgb:0000/0000/0000\x1b\\")
            answered = True
    elapsed = (time.perf_counter() - t0) * 1000
    os.kill(pid, 9)
    for _ in range(100):
        done, _ = os.waitpid(pid, os.WNOHANG)
        if done == pid:
            break
        time.sleep(0.02)
    os.close(fd)
    return elapsed


def boot_to_first_frame(runs=5, home=None):
    """Spawn to the banner reaching the terminal: what launch actually feels
    like. A `home` argument boots against a caller-built fixture; without one
    each run gets a fresh bare home."""
    samples = []
    for i in range(runs):
        fresh = home is None
        path = home or f"/tmp/ulo-bench-home-{os.getpid()}-{i}"
        samples.append(boot_sample(path))
        if fresh:
            shutil.rmtree(path, ignore_errors=True)
    return statistics.median(samples)


def loaded_boot(runs=5):
    """Boot against the populated home: the true cold start, extensions and
    all. The fixture is built once and reused — the measurements are about
    ulo's read set, not the fixture builder."""
    home = make_loaded_home()
    try:
        return boot_to_first_frame(runs, home)
    finally:
        shutil.rmtree(home, ignore_errors=True)


def cold_launch():
    """The binary's pages dropped from the file cache, then one full boot to
    first frame on a bare home: launching right after an update, when the
    binary must come off disk. A single sample — the floor, not a median."""
    subprocess.run([BINARY, "--version"], capture_output=True, check=True)
    evict_binary()
    home = f"/tmp/ulo-bench-cold-{os.getpid()}"
    try:
        return boot_sample(home)
    finally:
        shutil.rmtree(home, ignore_errors=True)


def make_session_home(i):
    """A home holding one heavy saved session for this workspace — 400 turns
    of prompt and markdown reply, the tail a unique marker — plus a fake
    provider sign-in so the boot goes straight to the restored transcript.
    Sessions are keyed by the cwd's sha256 slug; the benchmark spawns ulo with
    cwd = ROOT."""
    slug = "sha256-" + hashlib.sha256(os.path.realpath(ROOT).encode()).hexdigest()
    home = f"/tmp/ulo-bench-session-{os.getpid()}-{i}"
    sessions = os.path.join(home, "sessions", slug)
    os.makedirs(sessions)
    messages = [json.dumps({
        "type": "session", "format_version": 1,
        "id": "00000000-0000-0000-0000-000000000000",
        "cwd": os.path.realpath(ROOT), "created": 1788949125319, "model": "bench"})]
    for turn in range(400):
        messages.append(json.dumps({
            "type": "message", "id": f"u{turn}", "parent": None,
            "timestamp": 1788949125319 + turn,
            "message": {"role": "user",
                        "content": f"benchmark prompt {turn}: fix the flaky test in module {turn % 12}"}}))
        messages.append(json.dumps({
            "type": "message", "id": f"a{turn}", "parent": f"u{turn}",
            "timestamp": 1788949125319 + turn,
            "message": {"role": "assistant",
                        "content": "A **finished** response with some text.\n\n```rust\nfn main() {}\n```"}}))
    messages.append(json.dumps({
        "type": "message", "id": "tail", "parent": None,
        "timestamp": 1788949125319 + 999,
        "message": {"role": "assistant", "content": "resume-benchmark-tail"}}))
    stamp = int(time.time() * 1000)
    with open(os.path.join(sessions, f"{stamp}_00000000-0000-0000-0000-000000000000.jsonl"),
              "w", encoding="utf-8") as file:
        file.write("\n".join(messages) + "\n")
    with open(os.path.join(home, "auth.json"), "w", encoding="utf-8") as file:
        json.dump({"bench": {"ApiKey": {"key": "bench"}}}, file)
    return home


def session_resume(runs=5):
    """`ulo -c` on the heavy session: spawn → the restored tail painted. The
    cost of every 'continue where I left off', parse and rebuild included."""
    samples = []
    for i in range(runs):
        home = make_session_home(i)
        try:
            samples.append(boot_sample(home, marker=b"resume-benchmark-tail",
                                       argv=("ulo", "-c")))
        finally:
            shutil.rmtree(home, ignore_errors=True)
    return statistics.median(samples)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--build", action="store_true", help="rebuild the release binary")
    parser.add_argument("--check", action="store_true", help="enforce budgets without writing a report")
    args = parser.parse_args()
    if not os.path.exists(BINARY) or args.build:
        print("building release…", file=sys.stderr)
        build()
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT,
                            capture_output=True, text=True).stdout.strip()
    version = subprocess.run([BINARY, "--version"], capture_output=True,
                             text=True).stdout.strip()

    size = binary_size()
    cold = cold_start_version()
    boot = boot_to_first_frame()
    loaded = loaded_boot()
    chill = cold_launch()
    resume = session_resume()
    frame = long_session_frame()

    stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M")
    report = "\n".join([
        f"date:            {stamp}",
        f"version:         {version} ({commit})",
        f"machine:         {platform.machine()} · {platform.system()} {platform.release()}",
        f"binary size:     {size} bytes ({size / 1024 / 1024:.2f} MiB)",
        f"cold start:      {cold:.1f} ms   (ulo --version, median of 20)",
        f"first frame:     {boot:.1f} ms   (spawn → banner on a bare home, median of 5)",
        f"loaded boot:     {loaded:.1f} ms   "
        "(spawn → banner on a populated home — 30 skills, a package, 2 "
        "extensions, 1000 prompts — median of 5)",
        f"cold launch:     {chill:.1f} ms   "
        "(binary evicted from the file cache → first frame, single run)",
        f"session resume:  {resume:.1f} ms   "
        "(spawn → restored tail, ulo -c on a 400-turn session, median of 5)",
        f"long session:    {frame:.3f} ms/frame   (10,000 cached reply blocks, mean of 100)",
        "",
    ])
    out = os.path.join(ROOT, "benchmarks", "results", f"{stamp}_{commit}.txt")
    print(report)
    if args.check:
        with open(BUDGETS, encoding="utf-8") as file:
            budgets = json.load(file)
        measurements = {
            "binary_size_bytes": size,
            "cold_start_ms": cold,
            "first_frame_ms": boot,
            "loaded_first_frame_ms": loaded,
            "cold_launch_ms": chill,
            "resume_first_frame_ms": resume,
            "long_session_frame_ms": frame,
        }
        failures = [
            f"{name}: {measurements[name]:.1f} > {limit}"
            for name, limit in budgets.items()
            if measurements[name] > limit
        ]
        if failures:
            print("performance budget exceeded:", file=sys.stderr)
            for failure in failures:
                print(f"  {failure}", file=sys.stderr)
            return 1
        print("performance budgets: passed")
        return 0

    with open(out, "w", encoding="utf-8") as file:
        file.write(report)
    print(f"written: {os.path.relpath(out, ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
