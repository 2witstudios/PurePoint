"""Route component tests without skipping the required PR status."""

import json
import os
import subprocess
import sys
from pathlib import Path


def components(paths):
    shared = any(
        path == ".github/workflows/macos.yml" or path.startswith(".github/scripts/")
        for path in paths
    )
    packaged_rust = any(
        path.startswith("crates/") or path in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
        for path in paths
    )
    return {
        "macos": shared or any(
            path.startswith(("apps/purepoint-macos/", "crates/"))
            or path in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
            for path in paths
        ),
        "mobile": shared or packaged_rust or any(path.startswith("apps/purepoint-mobile/") for path in paths),
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


def require_checks(results):
    if results["changes"]["result"] != "success":
        raise ValueError("Changed-component detection did not succeed")
    for job, component in (("macos", "macos"), ("mobile-bridge", "mobile"), ("mobile-swift", "mobile")):
        flag = results["changes"]["outputs"].get(component)
        if flag not in ("true", "false"):
            raise ValueError("Missing component decision: " + component)
        expected = "success" if flag == "true" else "skipped"
        actual = results[job]["result"]
        if actual != expected:
            raise ValueError(f"{job}: expected {expected}, received {actual}")


if __name__ == "__main__":
    if sys.argv[1] == "changes":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        flags = components(changed_paths(event))
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for component, required in flags.items():
                print(f"{component}={str(required).lower()}", file=output)
                print(f"{component}: {'required' if required else 'unaffected'}")
    elif sys.argv[1] == "gate":
        require_checks(json.loads(os.environ["CI_RESULTS"]))
        print("Every applicable build and test job passed.")
    else:
        raise SystemExit("Usage: ci.py changes|gate")
