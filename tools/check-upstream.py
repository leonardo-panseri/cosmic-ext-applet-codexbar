#!/usr/bin/env python3
"""Report upstream CodexBar changes that may affect the adapter.

Compares the commit pinned in upstream.toml with upstream's default branch (or
--ref) for the files the applet depends on, and prints a Markdown report.

Exit status: 0 when nothing monitored changed, 1 when something changed,
2 on error. With --bump, writes the compared commit into upstream.toml instead.

Needs git and Python 3.11+ (stdlib only). Usage:

    python3 tools/check-upstream.py                  # report to stdout
    python3 tools/check-upstream.py --report out.md  # report to a file
    python3 tools/check-upstream.py --ref <sha> --bump
"""

import argparse
import difflib
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PIN_FILE = ROOT / "upstream.toml"

MONITORED_FILES = [
    "Integrations/Linux/main.cpp",
    "Integrations/Linux/DesktopController.cpp",
    "Integrations/Linux/Shared/Usage.js",
]
ICON_PATHSPEC = "Sources/CodexBar/Resources/ProviderIcon-*.svg"
README = "Integrations/Linux/README.md"
README_SECTION = "Adapter interface"

MARKER = "<!-- codexbar-upstream-monitor -->"
MAX_DIFF_LINES = 400
SHA_RE = re.compile(r"[0-9a-f]{40}")
STATUS_WORDS = {"A": "added", "D": "removed", "M": "modified", "T": "type changed"}


class CheckError(Exception):
    pass


def read_pin():
    pin = tomllib.loads(PIN_FILE.read_text())
    repo, commit = pin.get("repo"), pin.get("commit")
    if not isinstance(repo, str) or not re.fullmatch(r"[\w.-]+/[\w.-]+", repo):
        raise CheckError(f"{PIN_FILE.name}: `repo` must be owner/name")
    if not isinstance(commit, str) or not SHA_RE.fullmatch(commit):
        raise CheckError(f"{PIN_FILE.name}: `commit` must be a full 40-character SHA")
    return repo, commit


def git(gitdir, *args):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    result = subprocess.run(
        ["git", "--git-dir", str(gitdir), *args],
        capture_output=True, text=True, env=env,
    )
    if result.returncode != 0:
        raise CheckError(f"git {' '.join(args)} failed:\n{result.stderr.strip()}")
    return result.stdout


def fetch(gitdir, repo, pinned, ref):
    """Shallow, blobless fetch of the two commits; blobs are fetched on demand."""
    git(gitdir, "init", "--bare", "--quiet")
    git(gitdir, "remote", "add", "origin", f"https://github.com/{repo}.git")
    git(gitdir, "fetch", "--quiet", "--no-tags", "--depth=1", "--filter=blob:none", "origin",
        f"+{pinned}:refs/check/pinned", f"+{ref}:refs/check/upstream")
    return git(gitdir, "rev-parse", "refs/check/upstream").strip()


def default_branch(gitdir):
    out = git(gitdir, "ls-remote", "--symref", "origin", "HEAD")
    match = re.search(r"^ref: refs/heads/(\S+)\s+HEAD", out, re.M)
    return match.group(1) if match else "HEAD"


def changed_paths(gitdir):
    out = git(gitdir, "diff", "--no-renames", "--numstat", "-z",
              "refs/check/pinned", "refs/check/upstream", "--", *MONITORED_FILES, ICON_PATHSPEC)
    stats = {}
    fields = out.split("\0")
    for field in fields:
        if field:
            added, removed, path = field.split("\t", 2)
            stats[path] = (added, removed)
    status = git(gitdir, "diff", "--no-renames", "--name-status", "-z",
                 "refs/check/pinned", "refs/check/upstream", "--", *MONITORED_FILES, ICON_PATHSPEC)
    parts = [p for p in status.split("\0") if p]
    changes = []
    for code, path in zip(parts[0::2], parts[1::2]):
        changes.append((code, path, stats.get(path, ("-", "-"))))
    return changes


def read_section(text, title):
    """Return the lines of the `## title` section, or None if it is missing."""
    lines = text.splitlines()
    fence = None
    start = level = None
    for i, line in enumerate(lines):
        stripped = line.lstrip()
        if fence:
            if stripped.startswith(fence):
                fence = None
            continue
        if stripped.startswith(("```", "~~~")):
            fence = stripped[:3]
            continue
        heading = re.match(r"(#{1,6})\s+(.*?)\s*#*\s*$", line)
        if not heading:
            continue
        depth = len(heading.group(1))
        if start is None:
            if heading.group(2) == title:
                start, level = i, depth
        elif depth <= level:
            return lines[start:i]
    return lines[start:] if start is not None else None


def readme_section(gitdir, ref):
    try:
        text = git(gitdir, "show", f"{ref}:{README}")
    except CheckError:
        return None
    return read_section(text, README_SECTION)


def code_block(lines):
    if len(lines) > MAX_DIFF_LINES:
        lines = lines[:MAX_DIFF_LINES] + [f"... ({len(lines) - MAX_DIFF_LINES} more lines)"]
    # The section has its own code fences, so the outer fence must be longer.
    longest = max((len(run) for line in lines for run in re.findall(r"`+", line)), default=0)
    fence = "`" * max(3, longest + 1)
    return [f"{fence}diff", *lines, fence]


def report(repo, pinned, upstream, branch, changes, section_diff):
    web = f"https://github.com/{repo}"
    out = [
        MARKER,
        f"<!-- upstream-head: {upstream} -->",
        "Upstream CodexBar changed files the adapter depends on.",
        "",
        f"- Pinned: [`{pinned[:9]}`]({web}/commit/{pinned}) (from `upstream.toml`)",
        f"- Upstream{f' `{branch}`' if branch else ''}: [`{upstream[:9]}`]({web}/commit/{upstream})",
        f"- [Full upstream diff]({web}/compare/{pinned}...{upstream})",
        "",
    ]
    if changes:
        out += ["## Changed files", "", "| Change | File | +/− |", "|---|---|---|"]
        for code, path, (added, removed) in changes:
            ref = pinned if code == "D" else upstream
            word = STATUS_WORDS.get(code[0], code)
            out.append(f"| {word} | [`{path}`]({web}/blob/{ref}/{path}) | +{added} −{removed} |")
        out.append("")
    if section_diff is not None:
        out += [f"## README \"{README_SECTION}\" section", ""]
        out += section_diff
        out.append("")
    out += [
        "## Next steps",
        "",
        "Review the changes against the socket client, the snapshot types and the provider icons.",
        "Then record the reviewed commit and update the \"Upstream reference\" line in",
        "`docs/implementation-plan.md`:",
        "",
        "```sh",
        f"python3 tools/check-upstream.py --ref {upstream} --bump",
        "```",
        "",
        "This issue closes on the next run after the bump is on the default branch.",
    ]
    return "\n".join(out) + "\n"


def bump(commit):
    text = PIN_FILE.read_text()
    new, count = re.subn(r'(?m)^commit = "[^"]*"$', f'commit = "{commit}"', text)
    if count != 1:
        raise CheckError(f"{PIN_FILE.name}: expected exactly one `commit = \"...\"` line")
    PIN_FILE.write_text(new)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--ref", default="HEAD",
                        help="upstream branch or full SHA to compare with (default: the default branch)")
    parser.add_argument("--report", type=Path, help="write the report here instead of stdout")
    parser.add_argument("--bump", action="store_true",
                        help="write the compared upstream commit into upstream.toml")
    args = parser.parse_args()

    if not (args.ref == "HEAD" or SHA_RE.fullmatch(args.ref)
            or re.fullmatch(r"[\w./-]+", args.ref)):
        raise CheckError("--ref must be a branch name or a full SHA")
    repo, pinned = read_pin()

    with tempfile.TemporaryDirectory(prefix="codexbar-upstream-") as tmp:
        gitdir = Path(tmp) / "upstream.git"
        upstream = fetch(gitdir, repo, pinned, args.ref)
        if args.bump:
            bump(upstream)
            print(f"{PIN_FILE.name}: pinned {repo} at {upstream}")
            return 0
        if args.ref == "HEAD":
            branch = default_branch(gitdir)
        else:
            branch = None if SHA_RE.fullmatch(args.ref) else args.ref

        changes = [] if upstream == pinned else changed_paths(gitdir)
        old = readme_section(gitdir, "refs/check/pinned")
        new = readme_section(gitdir, "refs/check/upstream")
        if old is None:
            raise CheckError(f"the pinned {README} has no \"## {README_SECTION}\" section")
        section_diff = None
        if new is None:
            section_diff = [f"The section is missing from upstream `{README}` (removed or renamed)."]
        elif new != old:
            diff = difflib.unified_diff(old, new, f"a/{README}", f"b/{README}", lineterm="")
            section_diff = code_block(list(diff))

    changed = bool(changes) or section_diff is not None
    summary = (f"{repo} {pinned[:9]}..{upstream[:9]}: {len(changes)} monitored files changed, "
               f"README section {'changed' if section_diff else 'unchanged'}")
    text = report(repo, pinned, upstream, branch, changes, section_diff) if changed else ""
    if args.report:
        args.report.write_text(text)
        print(summary)
    else:
        print(text, end="")
        print(summary, file=sys.stderr)
    return 1 if changed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (CheckError, OSError, tomllib.TOMLDecodeError) as error:
        print(f"check-upstream: {error}", file=sys.stderr)
        sys.exit(2)
