# ACP probe: drives `opencode acp` over stdio JSON-RPC, logs every line.
# Usage: python probe.py <workdir> [--prompt]
import json, subprocess, sys, threading, time, os

WORKDIR = sys.argv[1].replace("/", "\\")
DO_PROMPT = "--prompt" in sys.argv
LOG = os.path.join(sys.argv[1], "wire.log")
EXE = r"C:\Users\29028\.version-fox\sdks\nodejs\node_modules\opencode-ai\bin\opencode.exe"

logf = open(LOG, "w", encoding="utf-8")
t0 = time.time()
results = {}   # id -> response
events = []    # notifications in order
done_init = threading.Event()
done_new = threading.Event()
first_agent_chunk = threading.Event()
prompt_done = threading.Event()
lock = threading.Lock()

def log(direction, obj):
    line = json.dumps(obj, ensure_ascii=False)
    with lock:
        logf.write(f"[{time.time()-t0:7.3f}s] {direction} {line}\n")
        logf.flush()

def stdout_reader(p):
    for raw in iter(p.stdout.readline, b""):
        if not raw.strip():
            continue
        try:
            msg = json.loads(raw.decode("utf-8"))
        except Exception as e:
            log("<-NONJSON", raw.decode("utf-8", "replace")[:500])
            continue
        log("<-", msg)
        if "id" in msg and ("result" in msg or "error" in msg):
            with lock:
                results[msg["id"]] = msg
            if msg["id"] == 0: done_init.set()
            elif msg["id"] == 1: done_new.set()
            elif msg["id"] == 2: prompt_done.set()
        elif msg.get("method") == "session/update":
            u = msg.get("params", {}).get("update", {})
            with lock:
                events.append(u)
            if u.get("sessionUpdate") == "agent_message_chunk":
                first_agent_chunk.set()
        elif "method" in msg and "id" in msg:
            # server -> client request (permission, fs...). auto-deny/cancel
            log("!!AGENT_REQUEST", {"id": msg["id"], "method": msg["method"]})
            resp = {"jsonrpc": "2.0", "id": msg["id"]}
            m = msg["method"]
            if m == "session/request_permission":
                resp["result"] = {"outcome": {"outcome": "cancelled"}}
            elif m == "fs/read_text_file":
                resp["error"] = {"code": -32601, "message": "not supported"}
            else:
                resp["error"] = {"code": -32601, "message": "not supported"}
            send(resp)

def stderr_reader(p):
    for raw in iter(p.stderr.readline, b""):
        s = raw.decode("utf-8", "replace").rstrip()
        if s:
            log("STDERR", s)

def send(obj):
    data = (json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8")
    log("->", obj)
    p.stdin.write(data)
    p.stdin.flush()

print("### spawning:", EXE, "cwd:", WORKDIR)
p = subprocess.Popen(
    [EXE, "acp"],
    cwd=WORKDIR,
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    creationflags=0x08000000,  # CREATE_NO_WINDOW
)
threading.Thread(target=stdout_reader, args=(p,), daemon=True).start()
threading.Thread(target=stderr_reader, args=(p,), daemon=True).start()

# 1. initialize
send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
    "protocolVersion": 1,
    "clientCapabilities": {"fs": {"readTextFile": False, "writeTextFile": False}},
    "clientInfo": {"name": "acp-probe", "version": "0.1.0"},
}})
if not done_init.wait(30):
    print("TIMEOUT initialize"); p.kill(); sys.exit(1)
init = results[0]
print("### INIT RESULT:")
print(json.dumps(init.get("result", init.get("error")), ensure_ascii=False, indent=1)[:3000])

# 2. session/new
send({"jsonrpc": "2.0", "id": 1, "method": "session/new", "params": {
    "cwd": WORKDIR, "mcpServers": [],
}})
if not done_new.wait(30):
    print("TIMEOUT session/new"); p.kill(); sys.exit(1)
new = results[1]
print("### SESSION/NEW RESULT:")
print(json.dumps(new.get("result", new.get("error")), ensure_ascii=False, indent=1)[:3000])

# 3. optional prompt turn + cancel
if DO_PROMPT and "result" in new:
    sid = new["result"]["sessionId"]
    send({"jsonrpc": "2.0", "id": 2, "method": "session/prompt", "params": {
        "sessionId": sid,
        "prompt": [{"type": "text", "text": "Reply with exactly the two characters: OK"}],
    }})
    # wait until first agent chunk or 25s, then cancel
    got = first_agent_chunk.wait(25)
    print("### first_agent_chunk:", got)
    if got and not prompt_done.is_set():
        time.sleep(0.5)
        send({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}})
    if not prompt_done.wait(20):
        print("TIMEOUT session/prompt response")
    else:
        print("### PROMPT RESULT:")
        print(json.dumps(results[2], ensure_ascii=False)[:1500])
    with lock:
        kinds = [e.get("sessionUpdate") for e in events]
    print("### UPDATE KINDS SEQUENCE:", kinds[:60])
    # print first chunk payloads trimmed
    with lock:
        for e in events[:8]:
            print("  EV:", json.dumps(e, ensure_ascii=False)[:400])

# 4. stdin close lifecycle test
print("### closing stdin; waiting up to 8s for exit")
try:
    p.stdin.close()
except Exception as e:
    print("stdin close err", e)
try:
    rc = p.wait(timeout=8)
    print("### EXIT rc =", rc)
except subprocess.TimeoutExpired:
    print("### DID NOT EXIT after stdin close; killing")
    p.kill()
    print("### EXIT rc after kill =", p.wait(timeout=5))
logf.close()
print("### wire log:", LOG)
