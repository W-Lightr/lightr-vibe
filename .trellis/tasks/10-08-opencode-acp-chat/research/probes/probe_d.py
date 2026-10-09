# Probe D: fake home, full model list, force a credential-needing model, capture prompt error shape
import json, subprocess, threading, time, os

EXE = r"C:\Users\29028\.version-fox\sdks\nodejs\node_modules\opencode-ai\bin\opencode.exe"
WORKDIR = r"C:\Users\29028\AppData\Local\Temp\acp-probe-work"
FAKEHOME = r"C:\Users\29028\AppData\Local\Temp\acp-probe-fakehome"

results = {}
p = subprocess.Popen([EXE, "acp"], cwd=WORKDIR, env={**os.environ, "USERPROFILE": FAKEHOME, "HOME": FAKEHOME,
    "XDG_CONFIG_HOME": os.path.join(FAKEHOME, ".config"), "XDG_DATA_HOME": os.path.join(FAKEHOME, ".local", "share")},
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, creationflags=0x08000000)
def reader():
    for raw in iter(p.stdout.readline, b""):
        if not raw.strip(): continue
        try: msg = json.loads(raw.decode())
        except Exception: continue
        if "id" in msg and ("result" in msg or "error" in msg):
            results[msg["id"]] = msg
        elif "method" in msg and "id" in msg:
            p.stdin.write((json.dumps({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32601,"message":"probe"}})+"\n").encode()); p.stdin.flush()
threading.Thread(target=reader, daemon=True).start()
def send(o): p.stdin.write((json.dumps(o)+"\n").encode()); p.stdin.flush()
def wait(i, t=60):
    end=time.time()+t
    while time.time()<end:
        if i in results: return results[i]
        time.sleep(0.05)
    return None

send({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"probe","version":"0"}}})
r0=wait(0)
print("authMethods:", json.dumps(r0["result"].get("authMethods"), ensure_ascii=False))
send({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":WORKDIR,"mcpServers":[]}})
r1=wait(1)
co={c["id"]:c for c in r1["result"]["configOptions"]}
sid=r1["result"]["sessionId"]
vals=[o["value"] for o in co["model"]["options"]]
print("ALL MODELS:", vals)
print("config ids:", list(co.keys()), "| effort options:", [o["value"] for o in co.get("effort",{}).get("options",[])])

# pick a credential-needing model if present, else try anthropic anyway
target = None
for pref in ["anthropic/", "openai/", "google/"]:
    for v in vals:
        if v.startswith(pref): target = v; break
    if target: break
if not target:
    target = "anthropic/claude-sonnet-4-5"  # guess; error shape is the point
print("TARGET:", target)
send({"jsonrpc":"2.0","id":2,"method":"session/set_model","params":{"sessionId":sid,"modelId":target}})
print("set_model:", json.dumps(wait(2,20))[:300])
send({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":sid,"prompt":[{"type":"text","text":"say hi"}]}})
r3=wait(3,60)
print("PROMPT ERR SHAPE:", json.dumps(r3, ensure_ascii=False)[:800])

# authenticate method shape (do NOT trigger browser): call authenticate with the advertised id and see response
send({"jsonrpc":"2.0","id":4,"method":"authenticate","params":{"methodId":"bogus-login"}})  # bogus id: error shape only, never triggers real login
r4=wait(4,25)
print("AUTHENTICATE:", json.dumps(r4, ensure_ascii=False)[:400])
p.stdin.close()
try: p.wait(timeout=8)
except Exception: p.kill()
print("exit", p.returncode)
