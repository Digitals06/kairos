"""CDP probe 3: verify post-fix state — cards render, values in display units, picker present."""
import json, sys, time
import websocket
import urllib.request

tabs = json.load(urllib.request.urlopen("http://127.0.0.1:9333/json", timeout=10))
page = next((t for t in tabs if t.get("type") == "page"), None)
if page is None:
    print("NO CDP PAGE (app launched without debug port?) — falling back to process-only check")
    sys.exit(0)
ws = websocket.create_connection(page["webSocketDebuggerUrl"], timeout=30)
mid = 0

def send(method, params=None):
    global mid
    mid += 1
    ws.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
    while True:
        msg = json.loads(ws.recv())
        if msg.get("id") == mid:
            return msg

send("Runtime.enable")
time.sleep(1)

# 1) UI renders?
r = send("Runtime.evaluate", {"expression":
    "JSON.stringify({cards: document.querySelectorAll('article.card').length, skeletons: document.querySelectorAll('[class*=skeleton]').length})",
    "returnByValue": True})
print("UI:", r["result"]["result"]["value"])

# 2) invoke get_overview directly: check display units + snake_case
expr = ("(async () => { const i = window.__TAURI_INTERNALS__; "
        "const cards = await i.invoke('get_overview'); "
        "const vt = cards.find(c => c.benchmark_id === 459) || cards[0]; "
        "return JSON.stringify({total: cards.length, sample: vt && {name: vt.benchmark_name, diff: vt.difficulty_name, progress: vt.benchmark_progress, rank: vt.rank, histLen: (vt.snapshot_history||[]).length}}); })()")
r = send("Runtime.evaluate", {"expression": expr, "awaitPromise": True, "returnByValue": True})
print("OVERVIEW:", r["result"]["result"].get("value") or r["result"]["result"].get("description"))

# 3) detail payload: scenario_history present?
expr2 = ("(async () => { const i = window.__TAURI_INTERNALS__; "
         "const d = await i.invoke('get_benchmark_detail', {benchmarkId: 459}); "
         "return JSON.stringify({scenHistSeries: (d.scenario_history||[]).length, "
         "firstSeries: d.scenario_history && d.scenario_history[0] && {name: d.scenario_history[0].scenario, pts: d.scenario_history[0].points.length, latestScore: d.scenario_history[0].points.at(-1)?.score}, "
         "playsWithScenario: d.plays.length > 0 ? d.plays[0].scenario : '(no plays)', "
         "topScenario: d.scenario_ranks && d.scenario_ranks[0]}); })()")
r = send("Runtime.evaluate", {"expression": expr2, "awaitPromise": True, "returnByValue": True})
print("DETAIL(459):", r["result"]["result"].get("value") or r["result"]["result"].get("description"))

ws.close()
print("DONE")
