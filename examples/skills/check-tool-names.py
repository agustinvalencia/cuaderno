#!/usr/bin/env python3
"""Check that every cuaderno tool an example skill names is advertised.

Builds the stdio MCP binary (`cdno-mcp`), asks it for its catalogue over
JSON-RPC (`initialize`, then `tools/list`) against an empty temporary vault
(a bare `.cuaderno/`),
and checks each backticked tool name in the given SKILL.md files against
that catalogue. With no arguments it checks `weekly-review/SKILL.md`.

A backticked span names a tool when its leading identifier is snake_case
and either sits in the first column of a table whose header has a `Tool`
column (on a row whose `Server` column, if any, is `cdno-mcp`), or starts
with a verb some advertised tool starts with (`get_`, `upsert_`, ...), so a
misspelt or removed tool is caught while response fields such as
`week_of` or `lapsed_habits` are not mistaken for tools. A span such as
`get_orientation.lapsed_habits` or `append_to_log(text)` is read by its
leading identifier.

Run from the repository root:

    python3 examples/skills/check-tool-names.py [path/to/SKILL.md ...]

Exits 0 when every named tool is advertised, 1 otherwise. Respects
CARGO_TARGET_DIR.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
IDENT = re.compile(r"^([a-z][a-z0-9]*(?:_[a-z0-9]+)+)(?=$|[(.])")
SPAN = re.compile(r"`([^`\n]+)`")


def advertised_catalogue():
    subprocess.run(
        ["cargo", "build", "--quiet", "-p", "cdno-mcp", "--bin", "cdno-mcp"],
        cwd=REPO,
        check=True,
    )
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    binary = target / "debug" / "cdno-mcp"
    with tempfile.TemporaryDirectory() as vault:
        (Path(vault) / ".cuaderno").mkdir()
        env =dict(os.environ, CUADERNO_VAULT_PATH=vault, RUST_LOG="off")
        proc = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=env,
            text=True,
        )

        def send(message):
            proc.stdin.write(json.dumps(message) + "\n")
            proc.stdin.flush()

        def response(request_id):
            for line in proc.stdout:
                message = json.loads(line)
                if message.get("id") == request_id:
                    if "error" in message:
                        sys.exit(f"cdno-mcp answered with an error: {message['error']}")
                    return message["result"]
            sys.exit("cdno-mcp closed its stdout before answering")

        try:
            send(
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "check-tool-names", "version": "0"},
                    },
                }
            )
            response(1)
            send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            names, cursor, request_id = set(), None, 2
            while True:
                params = {"cursor": cursor} if cursor else {}
                send({"jsonrpc": "2.0", "id": request_id, "method": "tools/list", "params": params})
                result = response(request_id)
                names.update(tool["name"] for tool in result["tools"])
                cursor = result.get("nextCursor")
                if not cursor:
                    return names
                request_id += 1
        finally:
            proc.stdin.close()
            proc.wait(timeout=10)


def named_tools(text, verbs):
    """Yield (line number, name, strict) for each span that names a tool."""
    tool_column = server_column = None
    for number, line in enumerate(text.splitlines(), start=1):
        cells = [c.strip() for c in line.strip().strip("|").split("|")] if line.lstrip().startswith("|") else None
        if cells is None:
            tool_column = server_column = None
        elif "Tool" in cells:
            tool_column = cells.index("Tool")
            server_column = cells.index("Server") if "Server" in cells else None
            continue
        elif tool_column is not None and not set(line.strip()) <= set("|-: "):
            ours = server_column is None or (
                server_column < len(cells) and cells[server_column] == "cdno-mcp"
            )
            if ours and tool_column < len(cells):
                for span in SPAN.findall(cells[tool_column]):
                    match = IDENT.match(span)
                    if match:
                        yield number, match.group(1), True
                line = "|".join(c for i, c in enumerate(cells) if i != tool_column)
        for span in SPAN.findall(line):
            match = IDENT.match(span)
            if match and match.group(1).split("_", 1)[0] in verbs:
                yield number, match.group(1), False


def main(argv):
    skills = [Path(p) for p in argv] or [HERE / "weekly-review" / "SKILL.md"]
    catalogue = advertised_catalogue()
    verbs = {name.split("_", 1)[0] for name in catalogue}
    print(f"advertised catalogue: {len(catalogue)} tools")
    failed = False
    for skill in skills:
        seen, missing = set(), []
        for number, name, _strict in named_tools(skill.read_text(encoding="utf-8"), verbs):
            seen.add(name)
            if name not in catalogue:
                missing.append((number, name))
        for number, name in missing:
            print(f"{skill}:{number}: `{name}` is not an advertised tool")
        failed |= bool(missing)
        status = "FAIL" if missing else "ok"
        print(f"{status}: {skill} names {len(seen)} tools: {', '.join(sorted(seen))}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
