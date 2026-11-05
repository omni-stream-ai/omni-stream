#!/usr/bin/env python3
"""Isolated MCP process used by Core and GUI integration tests."""
import json
import sys
import time
from pathlib import Path

root = Path(__file__).parent
names = ["click", "fill", "fill_form", "handle_dialog", "list_pages", "navigate_page",
         "new_page", "press_key", "select_page", "wait_for", "take_snapshot", "close_page"]
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    with (root / "mcp-calls.jsonl").open("a") as log:
        log.write(json.dumps({"method": method}) + "\n")
    if "id" not in request:
        continue
    mode_file = root / "browser-mode"
    mode = mode_file.read_text().strip() if mode_file.exists() else "ok"
    if method == "initialize":
        result = {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                  "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        (root / "startup-started").write_text("started")
        while (root / "startup-delay").exists():
            time.sleep(0.01)
        result = {"tools": [{"name": name, "inputSchema": {"type": "object", "properties": {}}}
                            for name in names if mode != "missing" or name != "take_snapshot"]}
    elif method == "tools/call":
        (root / "probe-started").write_text("started")
        while (root / "probe-delay").exists():
            time.sleep(0.01)
        result = {"isError": mode == "disconnected", "content": [{"type": "text", "text": "private-page-fixture"}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
