#!/usr/bin/env python3
"""Agent protocol fixture exercising real MCP relays or the generic command channel."""
import json
import shlex
import subprocess
import sys
import tomllib
from pathlib import Path

root = Path(__file__).parent
protocol = (root / "agent-protocol").read_text().strip()


def exercise(prompt, server=None):
    with (root / "agent-prompts.jsonl").open("a") as log:
        log.write(json.dumps(prompt) + "\n")
    (root / "agent-prompt.txt").write_text(prompt)
    (root / "agent-cwd.txt").write_text(str(Path.cwd()))
    (root / "relay-ready").write_text("ready")
    while (root / "relay-hold").exists():
        import time
        time.sleep(0.01)
    relay = None
    if server:
        relay = subprocess.Popen([server["command"], *server["args"]],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        sequence = 0

        def rpc(method, params):
            nonlocal sequence
            sequence += 1
            relay.stdin.write(json.dumps({"jsonrpc": "2.0", "id": sequence, "method": method, "params": params}) + "\n")
            relay.stdin.flush()
            value = json.loads(relay.stdout.readline())
            assert "error" not in value, value
            return value["result"]

        assert rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                  "clientInfo": {"name": "fixture", "version": "1"}})["capabilities"]["tools"] == {}
        tools = rpc("tools/list", {})
        invoke = lambda name, args: rpc("tools/call", {"name": name, "arguments": args})
    else:
        line = next(line for line in prompt.splitlines() if " plugin-tool --connection " in line and line.endswith(" list"))
        command = shlex.split(line)
        listed = subprocess.run(command, capture_output=True, text=True, check=True)
        tools = json.loads(listed.stdout)

        def invoke(name, args):
            result = subprocess.run([*command[:-1], "call", name], input=json.dumps(args), capture_output=True, text=True)
            return json.loads(result.stdout)

    (root / "tool-list.json").write_text(json.dumps(tools))
    names = [tool["name"] for tool in tools["tools"]]
    assert "chrome_evaluate_script" not in names
    assert "chrome_take_screenshot" not in names
    cases = json.loads((root / "tool-cases.json").read_text())
    results = []
    for case in cases:
        result = invoke(case["name"], case.get("arguments", {}))
        results.append(result)
        (root / "tool-results.json").write_text(json.dumps(results))
    (root / "tool-completed").write_text("complete")
    while (root / "agent-hold").exists():
        import time
        time.sleep(0.01)
    if relay:
        relay.stdin.close()
        assert relay.wait(timeout=3) == 0
    output = root / "agent-output.txt"
    return output.read_text() if output.exists() else "Universal plugin tools completed"


server = None
if protocol == "cli":
    if "--mcp-config" in sys.argv:
        server = json.loads(sys.argv[sys.argv.index("--mcp-config") + 1])["mcpServers"]["omnistream_plugins"]
    if "--fixture-mcp" in sys.argv:
        server = json.loads(Path(sys.argv[sys.argv.index("--fixture-mcp") + 1]).read_text())["mcpServers"]["omnistream_plugins"]
    print(exercise(sys.argv[-1], server))
elif protocol == "jsonl":
    json.loads(sys.stdin.readline())
    message = json.loads(sys.stdin.readline())
    if "--fixture-mcp" in sys.argv:
        server = json.loads(Path(sys.argv[sys.argv.index("--fixture-mcp") + 1]).read_text())["mcpServers"]["omnistream_plugins"]
    print(json.dumps({"type": "result", "id": "run", "output": exercise(message["input"]["goal"], server)}), flush=True)
elif protocol == "acp":
    for line in sys.stdin:
        message = json.loads(line)
        method = message["method"]
        if method == "initialize":
            result = {"protocolVersion": 1, "agentCapabilities": {}}
        elif method == "session/new":
            assert message["params"]["mcpServers"]
            server = message["params"]["mcpServers"][0]
            result = {"sessionId": "fixture-acp"}
        else:
            output = exercise(message["params"]["prompt"][0]["text"], server)
            print(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": "fixture-acp", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": output}}}}), flush=True)
            result = {"stopReason": "end_turn"}
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
else:
    config = next(arg for arg in sys.argv if arg.startswith("mcp_servers.omnistream_plugins="))
    server = tomllib.loads(config)["mcp_servers"]["omnistream_plugins"]
    active_thread = "fixture-app-server"
    for line in sys.stdin:
        message = json.loads(line)
        if "id" not in message:
            continue
        method = message["method"]
        if method in ("thread/start", "thread/resume", "thread/fork"):
            with (root / "thread-methods").open("a") as log:
                log.write(method + "\n")
            parent = message.get("params", {}).get("threadId")
            if method == "thread/resume" and parent == "fixture-app-server" and (root / "thread-busy").exists():
                print(json.dumps({"id": message["id"], "error": {"code": -32600, "message": f"thread {parent} already has an active writer"}}), flush=True)
                continue
            active_thread = "fixture-branch-app-server" if method == "thread/fork" else parent or "fixture-app-server"
            result = {"thread": {"id": active_thread}}
        elif method == "turn/start":
            output = exercise(message["params"]["input"][0]["text"], server)
            result = {"turn": {"id": "fixture-turn"}}
        else:
            result = {}
        print(json.dumps({"id": message["id"], "result": result}), flush=True)
        if method == "turn/start":
            print(json.dumps({"method": "item/completed", "params": {
                "threadId": active_thread, "turnId": "fixture-turn", "item": {"type": "agentMessage", "text": output}}}), flush=True)
            print(json.dumps({"method": "turn/completed", "params": {
                "threadId": active_thread, "turn": {"id": "fixture-turn", "status": "completed"}}}), flush=True)
