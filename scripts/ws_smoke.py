"""Drive the web app over its WebSocket protocol without a browser.

Plays a whole match by picking a random legal action each time, which exercises
the JSON round-trip in both directions (the client serialises `Action` back to
the server, so a broken enum representation shows up immediately).
"""
import asyncio, json, random, sys

import websockets

URL = sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8787/ws"
GAMES = int(sys.argv[2]) if len(sys.argv) > 2 else 1


def kind_of(tile):
    return tile >> 2


async def play_one(ws, game_no):
    await ws.send(json.dumps({"type": "new_game", "seat": random.randrange(4),
                              "length": "tonpuu", "bot": "efficiency",
                              "seed": 1000 + game_no}))
    steps = 0
    discards = 0
    while True:
        raw = await asyncio.wait_for(ws.recv(), timeout=30)
        msg = json.loads(raw)
        t = msg.get("type")
        if t == "error":
            print("  server error:", msg["message"]); return False
        if t == "game_end":
            return True
        if t != "state":
            continue
        dec = msg.get("decision")
        if not dec:
            continue
        acts = dec["actions"]
        pick = random.choice(acts)
        if "Discard" in pick if isinstance(pick, dict) else False:
            discards += 1
        await ws.send(json.dumps({"type": "action", "action": pick}))
        steps += 1
        if steps > 5000:
            print("  did not finish"); return False


async def main():
    ok = 0
    async with websockets.connect(URL) as ws:
        await ws.recv()  # initial state
        for g in range(GAMES):
            if await play_one(ws, g):
                ok += 1
    print(f"finished {ok}/{GAMES} games over the websocket protocol")
    return 0 if ok == GAMES else 1


sys.exit(asyncio.run(main()))
