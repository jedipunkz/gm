#!/usr/bin/env python3
"""Format and lint the project after edits to Rust source files."""

import json
import subprocess
import sys
from pathlib import Path


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(2)


try:
    event = json.load(sys.stdin)
except json.JSONDecodeError as error:
    fail(f"Could not read Claude Code hook input: {error}")

if not isinstance(event, dict):
    fail("Claude Code hook input must be a JSON object")

tool_input = event.get("tool_input")
if not isinstance(tool_input, dict):
    fail("Claude Code hook input has no tool_input object")

file_path = tool_input.get("file_path")
cwd = event.get("cwd")
if not isinstance(file_path, str) or not isinstance(cwd, str):
    fail("Claude Code hook input has no file_path or cwd")

if Path(file_path).suffix != ".rs":
    raise SystemExit(0)

root = Path(cwd).resolve()
edited = Path(file_path)
if not edited.is_absolute():
    edited = root / edited
edited = edited.resolve()
try:
    edited.relative_to(root)
except ValueError:
    raise SystemExit(0)

failed = False
for command in (
    ["cargo", "fmt"],
    ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
):
    try:
        result = subprocess.run(command, cwd=root, check=False)
    except OSError as error:
        print(f"Could not run {' '.join(command)}: {error}", file=sys.stderr)
        failed = True
        continue
    if result.returncode != 0:
        print(f"{' '.join(command)} failed with exit status {result.returncode}", file=sys.stderr)
        failed = True

if failed:
    raise SystemExit(2)
