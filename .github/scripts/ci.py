"""Route component tests without skipping the required PR status."""

import json
import os
import subprocess
import sys
from pathlib import Path


RUST_ROOT = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
LOCK_HELPER = {"crates/pu-cli/Cargo.toml", "crates/pu-cli/src/bin/point_guard_lock.rs"}
MOBILE = "apps/purepoint-mobile/"
RUNTIME_TARGETS = [
    {"runner": "macos-26", "architecture": "arm64"},
    {"runner": "macos-26-intel", "architecture": "x64"},
]


def components(paths, full=False):
    """Decide which jobs run. `full` (push to main, schedule, dispatch) runs everything."""
    if full:
        return {
            "macos": True, "mobile": True, "mobile_swift": True,
            "packaging": True, "release_archive": True, "universal": True,
            "runtime_targets": RUNTIME_TARGETS,
        }
    shared = any(
        path in {".github/workflows/macos.yml", ".github/workflows/release-proof.yml", ".github/workflows/release.yml"} or path.startswith((".github/scripts/", "apps/shared/ChatMarkdown/"))
        for path in paths
    )
    packaged_rust = any(path.startswith("crates/") or path in RUST_ROOT for path in paths)
    # Everything the iOS job compiles or runs: the app and its tests, the
    # verification checks, the bridge its trust/machine fixtures start, and
    # the lock helper those fixtures build.
    mobile_swift = shared or any(
        path.startswith((MOBILE + "PurePoint", MOBILE + "verification/", MOBILE + "bridge/"))
        or path in {
            MOBILE + "package.json", MOBILE + "package-lock.json",
            MOBILE + "runtime/verify-simulator-signing.py",
        } | LOCK_HELPER | RUST_ROOT
        for path in paths
    )
    # Inputs staged into the packaged runtime, including the lock helper it
    # signs and launches. Other Rust changes are proven by the ordinary jobs
    # and by full runs on main.
    packaging = shared or any(
        path.startswith((
            ".github/workflows/release", MOBILE + "runtime/", MOBILE + "support/", MOBILE + "docs/",
        ))
        or (path.startswith(MOBILE + "bridge/") and not path.endswith(".test.js"))
        or path in {MOBILE + "package.json", MOBILE + "package-lock.json"} | LOCK_HELPER
        or path.endswith(("/project.pbxproj", "/Package.resolved"))
        for path in paths
    )
    return {
        "macos": shared or any(
            path.startswith((
                MOBILE + "bridge/", MOBILE + "runtime/", MOBILE + "docs/", MOBILE + "support/",
                "apps/purepoint-macos/",
            )) or path in {MOBILE + "package.json", MOBILE + "package-lock.json"}
            for path in paths
        ) or packaged_rust,
        "mobile": shared or packaged_rust or any(path.startswith(MOBILE) for path in paths),
        "mobile_swift": mobile_swift,
        "packaging": packaging,
        # The only pull-request job that builds the macOS app in Release for
        # both architectures and verifies its deep signature.
        "release_archive": packaging or any(path.startswith("apps/purepoint-macos/") for path in paths),
        # The universal proof needs the Intel slice, which pull requests skip.
        "universal": False,
        "runtime_targets": RUNTIME_TARGETS[:1],
    }


def changed_paths(event):
    if "pull_request" in event:
        pr = event["pull_request"]
        revisions = [pr["base"]["sha"] + "..." + pr["head"]["sha"]]
    elif event["before"] == "0" * 40:
        return subprocess.check_output(
            ["git", "ls-tree", "-r", "--name-only", "-z", event["after"]]
        ).decode().split("\0")
    else:
        revisions = [event["before"], event["after"]]
    # Include both sides of renames and all files, avoiding GitHub path-filter limits.
    return subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z", *revisions, "--"]
    ).decode().split("\0")


CHECKS = (
    ("macos", "macos"),
    ("release-archive", "release_archive"),
    ("mobile-bridge", "mobile"),
    ("mobile-swift", "mobile_swift"),
    ("runtime-package", "packaging"),
    ("runtime-universal", "universal"),
    ("runtime-rust", "mobile"),
)


def detect(event_name, event_path):
    """Return step outputs; only pull requests are routed by changed paths."""
    if event_name == "pull_request":
        flags = components(changed_paths(json.loads(Path(event_path).read_text())))
    else:
        flags = components([], full=True)
    return {
        name: json.dumps(value) if name == "runtime_targets" else str(value).lower()
        for name, value in flags.items()
    }


def require_checks(results):
    if results["changes"]["result"] != "success":
        raise ValueError("Changed-component detection did not succeed")
    for job, component in CHECKS:
        flag = results["changes"]["outputs"].get(component)
        if flag not in ("true", "false"):
            raise ValueError("Missing component decision: " + component)
        expected = "success" if flag == "true" else "skipped"
        actual = results[job]["result"]
        if actual != expected:
            raise ValueError(f"{job}: expected {expected}, received {actual}")


if __name__ == "__main__":
    if sys.argv[1] == "changes":
        flags = detect(os.environ["GITHUB_EVENT_NAME"], os.environ["GITHUB_EVENT_PATH"])
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for component, value in flags.items():
                print(f"{component}={value}", file=output)
                print(f"{component}: {value}")
    elif sys.argv[1] == "gate":
        require_checks(json.loads(os.environ["CI_RESULTS"]))
        print("Every applicable build and test job passed.")
    else:
        raise SystemExit("Usage: ci.py changes|gate")
