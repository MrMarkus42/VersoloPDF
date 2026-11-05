#!/usr/bin/env python3
"""Downgrade dependencies in Cargo.lock that are younger than N days.

For every registry package that is too new, the newest semver-compatible
version that is old enough is selected with `cargo update --precise`.
Run `scripts/check-release-age.py` afterwards to verify the result.
"""

import argparse
import datetime as dt
import json
import pathlib
import subprocess
import sys
import time
import tomllib
import urllib.request

USER_AGENT = "versolopdf-release-age-check (https://github.com/MrMarkus42/VersoloPDF)"


def versions(name):
    req = urllib.request.Request(
        f"https://crates.io/api/v1/crates/{name}/versions?per_page=100",
        headers={"User-Agent": USER_AGENT},
    )
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = json.load(resp)
            break
        except OSError:
            if attempt == 3:
                raise
            time.sleep(5 * (attempt + 1))
    time.sleep(1.0)
    return [
        (v["num"], dt.datetime.fromisoformat(v["created_at"].replace("Z", "+00:00")))
        for v in data["versions"]
        if not v["yanked"]
    ]


def parse(v):
    core = v.split("+")[0]
    pre = "-" in core
    parts = core.split("-")[0].split(".")
    return tuple(int(p) for p in parts[:3]), pre


def compatible(a, b):
    """Cargo's caret compatibility of two versions."""
    (ma, mi, _), (mb, mib, _) = a, b
    if ma != 0 or mb != 0:
        return ma == mb
    return mi == mib


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--days", type=int, default=14)
    args = ap.parse_args()
    limit = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=args.days)

    lock = tomllib.loads(pathlib.Path("Cargo.lock").read_text(encoding="utf-8"))
    changed = False
    for pkg in lock.get("package", []):
        if not pkg.get("source", "").startswith("registry+"):
            continue
        name, current = pkg["name"], pkg["version"]
        all_versions = versions(name)
        cur_date = next((d for v, d in all_versions if v == current), None)
        if cur_date is None or cur_date <= limit:
            continue
        cur_v, _ = parse(current)
        candidates = [
            (parse(v)[0], v)
            for v, d in all_versions
            if d <= limit and not parse(v)[1] and compatible(parse(v)[0], cur_v) and parse(v)[0] < cur_v
        ]
        if not candidates:
            print(f"NO OLDER VERSION: {name}@{current}")
            continue
        # Newest first; older ones may violate the requirements of dependents.
        for _, target in sorted(candidates, reverse=True)[:6]:
            res = subprocess.run(
                ["cargo", "update", "-p", f"{name}@{current}", "--precise", target],
                capture_output=True,
                text=True,
            )
            if res.returncode == 0:
                print(f"{name}: {current} -> {target}")
                changed = True
                break
        else:
            print(f"NOT POSSIBLE: {name}@{current} (dependents need a newer version)")
    return 0 if changed else 1


if __name__ == "__main__":
    sys.exit(main())
