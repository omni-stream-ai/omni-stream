#!/usr/bin/env python3
"""App Server fixture that records the effective Codex launch configuration."""
import json
import sys
from pathlib import Path

root = Path(__file__).parent
with (root / "agent-args.jsonl").open("a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\n")

thread = "fixture-browser-thread"
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if "id" not in request:
        continue
    if method == "initialize":
        result = {}
    elif method in ("thread/start", "thread/resume"):
        thread = request.get("params", {}).get("threadId", thread)
        result = {"thread": {"id": thread}}
    elif method == "turn/start":
        with (root / "agent-turns.jsonl").open("a") as log:
            log.write(json.dumps(request["params"]) + "\n")
        result = {"turn": {"id": "fixture-turn"}}
    else:
        result = {}
    print(json.dumps({"id": request["id"], "result": result}), flush=True)
    if method == "turn/start":
        enabled = any(arg.startswith("mcp_servers.omnistream_plugins=") for arg in sys.argv)
        output = "Browser MCP enabled" if enabled else "Browser MCP disabled"
        print(json.dumps({"method": "item/agentMessage/delta", "params": {
            "threadId": thread, "turnId": "fixture-turn", "delta": output}}), flush=True)
        print(json.dumps({"method": "item/completed", "params": {
            "threadId": thread, "turnId": "fixture-turn", "item": {"type": "agentMessage", "text": output}}}), flush=True)
        print(json.dumps({"method": "turn/completed", "params": {
            "threadId": thread, "turn": {"id": "fixture-turn", "status": "completed"}}}), flush=True)
