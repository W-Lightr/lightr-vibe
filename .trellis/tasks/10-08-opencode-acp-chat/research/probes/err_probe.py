# Error-shape + set_config_option + stderr(--print-logs) probe
import json, subprocess, threading, time, os, re

WORKDIR = r"C:\Users\29028\AppData\Local\Temp\acp-probe-work"
EXE = r"C:\Users\29028\.version-fox\sdks\nodejs\node_modules\opencode-ai\bin\opencode.exe"
LOG = os.path.join(WORKDIR, "err-wire.log")
logf = open(LOG, "w", encoding="utf-8")
t0 = time.time(); results = {}; lock = threading.Lock()
ev = {}
def log(d,o):
    s=o if isinstance(o,str) else json.dumps(o,ensure_ascii=False)
    with lock: logf.write(f"[{time.time()-t0:7.3f}s] {d} {s}\n"); logf.flush()
def send(o):
    log("->",o); p.stdin.write((json.dumps(o,ensure_ascii=False)+"\n").encode()); p.stdin.flush()
def wait_id(i,t=30):
    end=time.time()+t
    while time.time()<end:
        with lock:
            if i in results: return results[i]
        time.sleep(0.05)
    return None
def reader(p):
    for raw in iter(p.stdout.readline,b""):
        if not raw.strip(): continue
        try: msg=json.loads(raw.decode())
        except Exception: log("<-NONJSON",raw.decode("utf-8","replace")[:200]); continue
        log("<-",msg)
        if "id" in msg and ("result" in msg or "error" in msg):
            with lock: results[msg["id"]]=msg
        elif "method" in msg and "id" in msg:
            send({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32601,"message":"probe: not supported"}})
def errreader(p):
    n=0
    for raw in iter(p.stderr.readline,b""):
        s=raw.decode("utf-8","replace").rstrip()
        if s: n+=1; log("STDERR",s)
        if n>40: break
    log("STDERR_COUNT",str(n))

p=subprocess.Popen([EXE,"acp","--print-logs","--log-level","INFO"],cwd=WORKDIR,
    stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=0x08000000)
threading.Thread(target=reader,args=(p,),daemon=True).start()
threading.Thread(target=errreader,args=(p,),daemon=True).start()

send({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"probe","version":"0"}}})
r0=wait_id(0); print("INIT caps:",json.dumps(r0["result"]["agentCapabilities"],ensure_ascii=False)[:300])
send({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":WORKDIR,"mcpServers":[]}})
r1=wait_id(1); sid=r1["result"]["sessionId"]; print("SID:",sid)

# 1) prompt with bogus sessionId
send({"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":"ses_bogus","prompt":[{"type":"text","text":"hi"}]}})
print("BOGUS SID ERR:",json.dumps(wait_id(2,20),ensure_ascii=False)[:400])

# 2) unknown method
send({"jsonrpc":"2.0","id":3,"method":"session/nonexistent","params":{}})
print("UNKNOWN METHOD:",json.dumps(wait_id(3,15),ensure_ascii=False)[:300])

# 3) invalid params (missing cwd)
send({"jsonrpc":"2.0","id":4,"method":"session/new","params":{"mcpServers":[]}})
print("MISSING CWD:",json.dumps(wait_id(4,15),ensure_ascii=False)[:300])

# 4) set_config_option: switch mode to plan
send({"jsonrpc":"2.0","id":5,"method":"session/set_config_option","params":{"sessionId":sid,"configId":"mode","value":"plan"}})
r5=wait_id(5,20)
if r5 and "result" in r5:
    print("SET_MODE OK, cur:",[ (c["id"],c.get("currentValue")) for c in r5["result"].get("configOptions",[])])
else:
    print("SET_MODE:",json.dumps(r5,ensure_ascii=False)[:400])

# 5) set_model variant (legacy name?) — see error shape
send({"jsonrpc":"2.0","id":6,"method":"session/set_model","params":{"sessionId":sid,"modelId":"opencode/exo-free"}})
print("SET_MODEL:",json.dumps(wait_id(6,15),ensure_ascii=False)[:300])

# 6) session/load of the same session (loadSession=true)
send({"jsonrpc":"2.0","id":7,"method":"session/load","params":{"sessionId":sid,"cwd":WORKDIR,"mcpServers":[]}})
print("SESSION/LOAD:",json.dumps(wait_id(7,25),ensure_ascii=False)[:400])

# 7) malformed JSON line
log("->","RAW:gibberish-not-json")
p.stdin.write(b"gibberish-not-json\n"); p.stdin.flush()
time.sleep(2)
p.stdin.close()
try: print("EXIT rc =",p.wait(timeout=8))
except subprocess.TimeoutExpired: p.kill(); print("KILLED rc =",p.wait(timeout=5))
logf.close()
# stderr summary
lines=open(LOG,encoding="utf-8").read().splitlines()
errs=[l for l in lines if "] STDERR " in l or "] STDERR_COUNT" in l]
print("STDERR lines:",len(errs))
for l in errs[:8]: print("  ",l[:200])
