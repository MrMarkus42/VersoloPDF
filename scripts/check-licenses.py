#!/usr/bin/env python3
"""Checks the licences of all dependencies and writes THIRD_PARTY_NOTICES.md.

Usage: scripts/check-licenses.py [--target TRIPLE ...] [--write]

Every package that ends up in a release build must be available under at
least one licence from ALLOWED. The script exits with status 1 otherwise.
"""

import argparse
import json
import re
import subprocess
import sys

ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
    "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0",
    "BSL-1.0", "0BSD", "OFL-1.1", "LicenseRef-UFL-1.0", "Ubuntu-font-1.0", "Unlicense", "NCSA",
}

TOKEN = re.compile(r"\(|\)|AND|OR|WITH\s+\S+|[A-Za-z0-9.+\-]+")


def satisfiable(expr: str) -> bool:
    """Evaluates an SPDX expression (also the legacy `A/B` form)."""
    expr = expr.replace("/", " OR ")
    tokens = TOKEN.findall(expr)
    pos = 0

    def parse_or():
        nonlocal pos
        value = parse_and()
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            value = parse_and() or value
        return value

    def parse_and():
        nonlocal pos
        value = parse_atom()
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            value = parse_atom() and value
        return value

    def parse_atom():
        nonlocal pos
        tok = tokens[pos]
        pos += 1
        if tok == "(":
            value = parse_or()
            pos += 1  # ')'
            return value
        if pos < len(tokens) and tokens[pos].startswith("WITH"):
            tok = f"{tok} {tokens[pos]}"
            pos += 1
        return tok in ALLOWED

    return parse_or()


def packages(targets):
    pkgs = {}
    for target in targets or [None]:
        cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
        if target:
            cmd += ["--filter-platform", target]
        meta = json.loads(subprocess.check_output(cmd))
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        by_id = {p["id"]: p for p in meta["packages"]}
        root = meta["resolve"]["root"]
        # Walk only normal (non-dev, non-build) dependencies of the root.
        stack, seen = [root], set()
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            for dep in nodes[pid]["deps"]:
                kinds = {k["kind"] for k in dep["dep_kinds"]}
                if pid == root and kinds <= {"dev"}:
                    continue
                if kinds <= {"dev"}:
                    continue
                stack.append(dep["pkg"])
        for pid in seen:
            p = by_id[pid]
            if pid != root:
                pkgs[(p["name"], p["version"])] = p
    return pkgs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", action="append", default=[])
    ap.add_argument("--write", action="store_true")
    args = ap.parse_args()

    pkgs = packages(args.target)
    bad = []
    rows = []
    for (name, version), p in sorted(pkgs.items()):
        lic = p.get("license") or ""
        if not lic or not satisfiable(lic):
            bad.append(f"{name} {version}: {lic or '(no licence field)'}")
        rows.append(f"| {name} | {version} | {lic} | {p.get('repository') or ''} |")

    if args.write:
        with open("THIRD_PARTY_NOTICES.md", "w", encoding="utf-8") as f:
            f.write("# Third-party software\n\n")
            f.write(
                "MrPDF uses the following components. Their licence texts are\n"
                "included in their source distributions (crates.io) and, for\n"
                "PDFium, in the `licenses/` folder shipped next to the program.\n\n"
            )
            f.write("## PDFium\n\n")
            f.write(
                "PDF rendering engine by Google and the PDFium authors, prebuilt by\n"
                "https://github.com/bblanchon/pdfium-binaries (release chromium/7881).\n"
                "Licence: BSD-3-Clause and Apache-2.0; bundled third-party code\n"
                "(FreeType, libjpeg-turbo, OpenJPEG, LittleCMS, zlib, libpng, libtiff, ICU,\n"
                "Abseil, AGG 2.3, fast_float, simdutf, LLVM libc) under their respective\n"
                "permissive licences.\n\n"
            )
            f.write("## Rust crates\n\n| Crate | Version | Licence | Repository |\n|---|---|---|---|\n")
            f.write("\n".join(rows) + "\n")

    if bad:
        print("Packages with licences outside the allow list:")
        print("\n".join(bad))
        return 1
    print(f"OK: {len(pkgs)} packages, all licences allowed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
