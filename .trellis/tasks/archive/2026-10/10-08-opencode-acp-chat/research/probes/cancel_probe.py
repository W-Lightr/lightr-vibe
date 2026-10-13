# Cancel-semantics probe: send a long-output prompt, cancel after 2s, observe stopReason.
import json, subprocess, sys, threading, time, os

WORKDIR = r"C:\Users\29028\AppData\Local\Temp\acp-probe-work"
EXE = r"C:\Users\29028\.version-fox\sdks\nodejs\node_modules\opencode-ai\bin\opencode.exe"
LOG = os.path.join(WORKDIR, "cancel-wire.log")
logf = open(LOG, "w", encoding="utf-8")
t0 = time.time()
results = {}
events = []
done_init = threading.Event(); done_new = threading.Event(); prompt_done = threading.Event()
lock = threading.Lock()

def log(d, obj):
    s = obj if isinstance(obj, str) else json.dumps(obj, ensure_ascii=False)
    with lock:
        logf.write(f"[{time.time()-t0:7.3f}s] {d} {s}\n"); logf.flush()

def send(obj):
    log("->", obj)
    p.stdin.write((json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8")); p.stdin.flush()

def reader(p):
    for raw in iter(p.stdout.readline, b""):
        if not raw.strip(): continue
        try: msg = json.loads(raw.decode("utf-8"))
        except Exception: log("<-NONJSON", raw.decode("utf-8","replace")[:300]); continue
        log("<-", msg)
        if "id" in msg and ("result" in msg or "error" in msg):
            with lock: results[msg["id"]] = msg
            {0: done_init, 1: done_new, 2: prompt_done}.get(msg["id"], threading.Event()).set()
        elif msg.get("method") == "session/update":
            with lock: events.append(msg["params"]["update"])
        elif "method" in msg and "id" in msg:
            log("!!AGENT_REQUEST", {"id": msg["id"], "method": msg["method"]})
            send({"jsonrpc":"2.0","id":msg["id"],"result":{"outcome":{"outcome":"cancelled"}}} if msg["method"]=="session/request_permission" else {"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32601,"message":"not supported"}})

p = subprocess.Popen([EXE, "acp"], cwd=WORKDIR, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=0x08000000)
threading.Thread(target=reader, args=(p,), daemon=True).start()

send({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":False,"writeTextFile":False}},"clientInfo":{"name":"acp-probe","version":"0.1.0"}}})
done_init.wait(30)
send({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":WORKDIR,"mcpServers":[]}})
done_new.wait(30)
sid = results[1]["result"]["sessionId"]

send({"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":sid,"prompt":[{"type":"text","text":"Count slowly from 1 to 500, one number per line, with a short comment on each number."}]}})
time.sleep(3.0)
print("### sending session/cancel after 3s; events so far:", len(events))
send({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":sid}})
ok = prompt_done.wait(20)
print("### prompt response arrived:", ok)
print("### PROMPT RESULT:", json.dumps(results.get(2), ensure_ascii=False)[:800])
with lock:
    print("### KINDS:", [e.get("sessionUpdate") for e in events][:40])
    print("### N events:", len(events))
# also test session/cancel when idle (no active turn) — should be safe no-op
send({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":sid}})
time.sleep(1.5)
# close
try: p.stdin.close()
except Exception as e: print("stdin err", e)
try: print("### EXIT rc =", p.wait(timeout=8))
except subprocess.TimeoutExpired: p.kill(); print("### killed rc =", p.wait(timeout=5))
logf.close()
