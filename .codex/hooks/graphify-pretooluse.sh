#!/usr/bin/env bash
set -euo pipefail

if [ ! -f graphify-out/graph.json ]; then
  exit 0
fi

payload="$(cat)"
decision="$(
  printf '%s' "$payload" | python3 -c '
import json
import shlex
import sys

READ_TOOLS = {
    "Read",
    "Grep",
    "Glob",
    "LS",
    "FileRead",
    "read_file",
    "list_directory",
    "search_files",
}

READ_COMMANDS = {
    "rg",
    "ripgrep",
    "grep",
    "find",
    "fd",
    "ls",
    "tree",
    "cat",
    "sed",
    "awk",
    "head",
    "tail",
    "nl",
    "bat",
    "less",
    "more",
    "wc",
    "du",
    "stat",
    "file",
}

GIT_READ_SUBCOMMANDS = {
    "show",
    "diff",
    "grep",
    "ls-files",
    "status",
    "log",
    "blame",
}

try:
    data = json.load(sys.stdin)
except json.JSONDecodeError:
    sys.exit(0)

tool_name = data.get("tool_name") or data.get("tool") or data.get("name")
if tool_name in READ_TOOLS:
    print("warn")
    sys.exit(0)

tool_input = data.get("tool_input", data)
command = tool_input.get("command", "")
if not command:
    sys.exit(0)

try:
    tokens = shlex.split(command)
except ValueError:
    tokens = command.split()

for index, token in enumerate(tokens):
    if token in READ_COMMANDS:
        print("warn")
        sys.exit(0)
    if token == "git" and index + 1 < len(tokens) and tokens[index + 1] in GIT_READ_SUBCOMMANDS:
        print("warn")
        sys.exit(0)
' 2>/dev/null || true
)"

if [ "$decision" != "warn" ]; then
  exit 0
fi

echo '{"systemMessage":"graphify: Knowledge graph exists. Read graphify-out/GRAPH_REPORT.md for god nodes and community structure before searching raw files."}'
