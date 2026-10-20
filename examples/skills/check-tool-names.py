#!/usr/bin/env python3
"""Check that every cuaderno tool and field an example skill names exists.

Builds the stdio MCP binary (`cdno-mcp`), asks it for its catalogue over
JSON-RPC (`initialize`, then `tools/list`) against an empty temporary vault
(a bare `.cuaderno/`),
and checks each backticked tool name in the given SKILL.md files against
that catalogue. With no arguments it checks `daily-orientation/SKILL.md`,
`weekly-review/SKILL.md` and `references/ADHD-PRINCIPLES.md`.

A backticked span names a tool when its leading identifier is snake_case
and either sits in the first column of a table whose header has a `Tool`
column (on a row whose `Server` column, if any, is `cdno-mcp`), or starts
with a verb some advertised tool starts with (`get_`, `upsert_`, ...), so a
misspelt or removed tool is caught while response fields such as
`week_of` or `lapsed_habits` are not mistaken for tools. A span such as
`get_orientation.lapsed_habits` or `append_to_log(text)` is read by its
leading identifier.

Every other backticked span shaped like a field reference is checked too:

- a path (`slots.cap`, `projects[].top_action.energy`,
  `get_orientation.slots.{active,counted,cap}`) must start at a tool or a
  field, and each later segment must be a field of a DTO;
- a shape (`{ slug, counts, held_by }`) must list only DTO fields;
- a call (`hand_off_project(project, take_back: true)`) must pass only
  parameters in that tool's advertised input schema;
- a bare identifier (`counts`, `focus_open`), with or without a trailing
  `: value`, must be a DTO field, a tool parameter, a tool, a string
  literal in `crates/cdno-mcp/src` (enum values), a rejection code (the
  `RejectionCode` variants in `rejection.rs`), a JSON literal, a config
  key in `crates/cdno-core/src/config.rs`, or one of the daily-log marker
  words in LOG_WORDS.

DTO fields are read from `crates/cdno-mcp/src/dto.rs`: every `pub name:`,
every serde `rename`, `tag` and `content` name, and the snake_case names of
enum variants. Spans with spaces, capitals or punctuation outside these
shapes (`Standup`, `deep | medium | light`, `[[wikilinks]]`) are prose and
are skipped.

Run from the repository root:

    python3 examples/skills/check-tool-names.py [path/to/SKILL.md ...]

Exits 0 when every named tool and field exists, 1 otherwise. Respects
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
WORD = r"[a-z_][a-z0-9_]*"
PATH = re.compile(rf"^{WORD}(?:\[\])?(?:\.(?:{WORD}|\{{[a-z0-9_, ]+\}})(?:\[\])?)*$")
CALL = re.compile(rf"^({WORD})\((.*)\)$")
# Daily-log marker words the skills quote as the log's own vocabulary.
LOG_WORDS = {"started", "resumed", "paused", "during"}
MCP_SRC = REPO / "crates" / "cdno-mcp" / "src"
CONFIG_RS = REPO / "crates" / "cdno-core" / "src" / "config.rs"


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
            tools, cursor, request_id = {}, None, 2
            while True:
                params = {"cursor": cursor} if cursor else {}
                send({"jsonrpc": "2.0", "id": request_id, "method": "tools/list", "params": params})
                result = response(request_id)
                for tool in result["tools"]:
                    tools[tool["name"]] = schema_properties(tool.get("inputSchema", {}))
                cursor = result.get("nextCursor")
                if not cursor:
                    return tools
                request_id += 1
        finally:
            proc.stdin.close()
            proc.wait(timeout=10)


def schema_properties(schema):
    """Every property name anywhere in a JSON Schema."""
    names = set()
    if isinstance(schema, dict):
        for key, value in schema.items():
            if key == "properties" and isinstance(value, dict):
                names.update(value)
            names |= schema_properties(value)
    elif isinstance(schema, list):
        for value in schema:
            names |= schema_properties(value)
    return names


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def dto_fields():
    """Field names an agent can see in a response, read from dto.rs."""
    text = (MCP_SRC / "dto.rs").read_text(encoding="utf-8")
    fields = set(re.findall(r"^\s*pub (" + WORD + r"):", text, re.M))
    fields |= set(re.findall(r'serde\([^)]*\b(?:rename|tag|content) = "(\w+)"', text))
    for body in re.findall(r"pub enum \w+ \{(.*?)^\}", text, re.S | re.M):
        fields |= {snake(v) for v in re.findall(r"^\s*([A-Z]\w*)\s*[,({]", body, re.M)}
    return fields


def known_words():
    """Bare words that are neither tools nor fields but exist in the code."""
    words = set()
    for path in MCP_SRC.rglob("*.rs"):
        words |= set(re.findall(r'"(' + WORD + r')"', path.read_text(encoding="utf-8")))
    rejection = (MCP_SRC / "rejection.rs").read_text(encoding="utf-8")
    for body in re.findall(r"enum RejectionCode \{(.*?)^\}", rejection, re.S | re.M):
        words |= {snake(v) for v in re.findall(r"^\s*([A-Z]\w*)\s*,", body, re.M)}
    words |= set(re.findall(r"^\s*pub (" + WORD + r"):", CONFIG_RS.read_text(encoding="utf-8"), re.M))
    return words | LOG_WORDS | {"null", "true", "false"}


def check_span(span, tools, fields, words):
    """Return (kind, problems) for a span, or None when it is prose."""
    span = span.strip()
    call = CALL.match(span)
    if call:
        name, args = call.groups()
        if name not in tools:
            return None  # a non-tool call (or a misspelt tool, caught above)
        params = [re.match(r"\s*(" + WORD + r")", a) for a in args.split(",") if a.strip()]
        bad = [m.group(1) for m in params if m and m.group(1) not in tools[name]]
        return "call", [f"`{b}` is not a parameter of `{name}`" for b in bad]
    if span.startswith("{") and span.endswith("}"):
        body = re.sub(r'"[^"]*"', "", span)
        names = set(re.findall(WORD, body)) - {"null", "true", "false"}
        return "shape", [f"`{n}` is not a DTO field" for n in sorted(names - fields)]
    head = re.split(r":\s", span, maxsplit=1)[0] if re.search(r":\s", span) else span
    if not PATH.match(head):
        return None
    segments = re.split(r"\.", head.replace("[]", ""))
    if len(segments) == 1:
        word = segments[0]
        ok = word in fields or word in words or word in tools or any(word in p for p in tools.values())
        return "word", [] if ok else [f"`{word}` is not a DTO field, parameter, tool or known value"]
    problems = []
    first, rest = segments[0], segments[1:]
    if first not in tools and first not in fields:
        problems.append(f"`{first}` is neither a tool nor a DTO field")
    for segment in rest:
        for name in re.findall(WORD, segment):
            if name not in fields:
                problems.append(f"`{name}` (in `{head}`) is not a DTO field")
    return "path", problems


def named_tools(text, verbs, foreign=None):
    """Yield (line number, name, strict) for each span that names a tool.

    Tool names in a table row whose `Server` is not `cdno-mcp` are added to
    `foreign`, when given, so the field check skips them.
    """
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
            if not ours and foreign is not None and tool_column < len(cells):
                foreign.update(SPAN.findall(cells[tool_column]))
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
    skills = [Path(p) for p in argv] or [
        HERE / "daily-orientation" / "SKILL.md",
        HERE / "weekly-review" / "SKILL.md",
        HERE / "references" / "ADHD-PRINCIPLES.md",
    ]
    catalogue = advertised_catalogue()
    fields, words = dto_fields(), known_words()
    verbs = {name.split("_", 1)[0] for name in catalogue}
    print(f"advertised catalogue: {len(catalogue)} tools; dto.rs: {len(fields)} field names")
    failed = False
    for skill in skills:
        text = skill.read_text(encoding="utf-8")
        seen, missing, foreign = set(), [], set()
        for number, name, _strict in named_tools(text, verbs, foreign):
            seen.add(name)
            if name not in catalogue:
                missing.append((number, name))
        for number, name in missing:
            print(f"{skill}:{number}: `{name}` is not an advertised tool")
        checked, problems = set(), []
        for number, line in enumerate(text.splitlines(), start=1):
            for span in SPAN.findall(line):
                if span in foreign or span in catalogue:
                    continue
                result = check_span(span, catalogue, fields, words)
                if result is None:
                    continue
                checked.add(span.strip())
                problems += [(number, p) for p in result[1]]
        for number, problem in problems:
            print(f"{skill}:{number}: {problem}")
        failed |= bool(missing) or bool(problems)
        status = "FAIL" if missing or problems else "ok"
        print(f"{status}: {skill} names {len(seen)} tools: {', '.join(sorted(seen))}")
        print(f"{status}: {skill} checks {len(checked)} field, parameter and value spans")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
