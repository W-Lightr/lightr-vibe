# Probe C: verify session/set_model effect; Probe D: no-credential failure shape (isolated USERPROFILE)
import json, subprocess, threading, time, os, sys

EXE = r"C:\Users\29028\.version-fox\sdks\nodejs\node_modules\opencode-ai\bin\opencode.exe"
WORKDIR = r"C:\Users\29028\AppData\Local\Temp\acp-probe-work"
FAKEHOME = r"C:\Users\29028\AppData\Local\Temp\acp-probe-fakehome"

def run(env=None, steps=None, label=""):
    results = {}
    p = subprocess.Popen([EXE, "acp"], cwd=WORKDIR, env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=0x08000000)
    def reader():
        for raw in iter(p.stdout.readline, b""):
            if not raw.strip(): continue
            try: msg = json.loads(raw.decode())
            except Exception: continue
            if "id" in msg and ("result" in msg or "error" in msg):
                results[msg["id"]] = msg
            elif "method" in msg and "id" in msg:
                try:
                    p.stdin.write((json.dumps({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32601,"message":"probe"}})+"\n").encode()); p.stdin.flush()
                except Exception: pass
    threading.Thread(target=reader, daemon=True).start()
    def send(o):
        p.stdin.write((json.dumps(o)+"\n").encode()); p.stdin.flush()
    def wait(i, t=40):
        end = time.time()+t
        while time.time() < end:
            if i in results: return results[i]
            time.sleep(0.05)
        return None
    send({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"probe","version":"0"}}})
    r0 = wait(0)
    print(label, "INIT:", "ok" if r0 and "result" in r0 else json.dumps(r0)[:200])
    steps(send, wait, label)
    p.stdin.close()
    try: p.wait(timeout=8)
    except Exception: p.kill()

def steps_a(send, wait, label):
    # real config: does set_model take effect?
    send({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":WORKDIR,"mcpServers":[]}})
    r1 = wait(1); sid = r1["result"]["sessionId"]
    send({"jsonrpc":"2.0","id":2,"method":"session/set_model","params":{"sessionId":sid,"modelId":"opencode/exo-free"}})
    print(label, "set_model resp:", json.dumps(wait(2))[:200])
    # force a configOptions refresh via set_config_option(mode) — response carries current model value
    send({"jsonrpc":"2.0","id":3,"method":"session/set_config_option","params":{"sessionId":sid,"configId":"mode","value":"build"}})
    r3 = wait(3)
    if r3 and "result" in r3:
        cur = {c["id"]: c.get("currentValue") for c in r3["result"].get("configOptions", [])}
        print(label, "after set_model -> currentValues:", cur)
    else:
        print(label, "set_config_option:", json.dumps(r3)[:300])
    # also try spec session/set_mode
    send({"jsonrpc":"2.0","id":4,"method":"session/set_mode","params":{"sessionId":sid,"modeId":"plan"}})
    print(label, "set_mode resp:", json.dumps(wait(4))[:300])

def steps_b(send, wait, label):
    # fake empty home: session/new + prompt failure shape
    send({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":WORKDIR,"mcpServers":[]}})
    r1 = wait(1, 40)
    print(label, "session/new:", json.dumps(r1)[:600])
    if not (r1 and "result" in r1): return
    sid = r1["result"]["sessionId"]
    send({"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":sid,"prompt":[{"type":"text","text":"say hi"}]}})
    r2 = wait(2, 60)
    print(label, "prompt resp:", json.dumps(r2)[:600])

os.makedirs(FAKEHOME, exist_ok=True)
run(steps=steps_a, label="[A real-home]")
env = dict(os.environ)
env["USERPROFILE"] = FAKEHOME
env["HOME"] = FAKEHOME
env["XDG_CONFIG_HOME"] = os.path.join(FAKEHOME, ".config")
env["XDG_DATA_HOME"] = os.path.join(FAKEHOME, ".local", "share")
run(env=env, steps=steps_b, label="[B fake-home]")
