#!/usr/bin/env python3
"""Check that every crates.io dependency in Cargo.lock is at least N days old.

Usage: scripts/check-release-age.py [--days 14] [--lock Cargo.lock]

Freshly published crate versions are a common vector for supply-chain
attacks, so this project only accepts releases that have been public for a
while. The script prints offending packages and exits with status 1 if any
are found. A small on-disk cache avoids repeated API requests.
"""

import argparse
import datetime as dt
import json
import pathlib
import sys
import time
import tomllib
import urllib.request

API = "https://crates.io/api/v1/crates/{name}/{version}"
USER_AGENT = "versolopdf-release-age-check (https://github.com/MrMarkus42/VersoloPDF)"
CACHE = pathlib.Path.home() / ".cache" / "versolopdf-release-dates.json"


def fetch_date(name: str, version: str) -> str:
    req = urllib.request.Request(
        API.format(name=name, version=version), headers={"User-Agent": USER_AGENT}
    )
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = json.load(resp)
            return data["version"]["created_at"]
        except OSError:
            if attempt == 3:
                raise
            time.sleep(5 * (attempt + 1))
    raise AssertionError("unreachable")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--days", type=int, default=14)
    parser.add_argument("--lock", default="Cargo.lock")
    args = parser.parse_args()

    lock = tomllib.loads(pathlib.Path(args.lock).read_text(encoding="utf-8"))
    cache = {}
    if CACHE.exists():
        cache = json.loads(CACHE.read_text(encoding="utf-8"))

    limit = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=args.days)
    too_new = []
    for pkg in lock.get("package", []):
        source = pkg.get("source", "")
        if not source.startswith("registry+"):
            continue
        key = f"{pkg['name']}@{pkg['version']}"
        if key not in cache:
            cache[key] = fetch_date(pkg["name"], pkg["version"])
            time.sleep(1.0)  # crates.io crawler policy: max one request per second
        created = dt.datetime.fromisoformat(cache[key].replace("Z", "+00:00"))
        if created > limit:
            too_new.append((key, cache[key][:10]))

    CACHE.parent.mkdir(parents=True, exist_ok=True)
    CACHE.write_text(json.dumps(cache, indent=0, sort_keys=True), encoding="utf-8")

    for key, date in too_new:
        print(f"TOO NEW: {key} ({date})")
    if too_new:
        return 1
    print(f"OK: all registry packages are older than {args.days} days")
    return 0


if __name__ == "__main__":
    sys.exit(main())
