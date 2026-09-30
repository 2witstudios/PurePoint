#!/usr/bin/env python3
"""Slow-rendering fake of Claude Code's input box, for delivery tests.

Knobs (env):
  READY_DELAY_MS    keystrokes are discarded and no box is drawn until this
                    elapses (models Claude still loading hooks / first render)
  PROCESS_DELAY_MS  each typed character becomes visible this long after it
                    arrives (models a loaded machine); Enter is handled
                    immediately on arrival, so it can submit a fragment
  SWALLOW_ENTER     number of leading Enter presses to ignore
  SUBMIT_LOG        file that receives one JSON line per submitted turn
"""
import json, os, select, sys, termios, threading, time, tty

READY_DELAY = int(os.environ.get("READY_DELAY_MS", "0")) / 1000
PROCESS_DELAY = int(os.environ.get("PROCESS_DELAY_MS", "0")) / 1000
swallow = int(os.environ.get("SWALLOW_ENTER", "0"))
log_path = os.environ.get("SUBMIT_LOG")
COLS = 60

fd = sys.stdin.fileno()
tty.setraw(fd)
out = sys.stdout


def emit(s):
    out.write(s)
    out.flush()


transcript = []
buf = []
lock = threading.Lock()


def draw():
    text = "".join(buf)
    rows = ["\x1b[2J\x1b[H"]
    for t in transcript[-8:]:
        rows.append("\x1b[1m> " + t + "\x1b[0m\r\n")
    rows.append("╭" + "─" * (COLS - 2) + "╮\r\n")
    chunks = [text[i:i + COLS - 6] for i in range(0, len(text), COLS - 6)] or [""]
    for i, c in enumerate(chunks):
        mark = ">" if i == 0 else " "
        rows.append("│ " + mark + " " + c.ljust(COLS - 6) + " │\r\n")
    rows.append("╰" + "─" * (COLS - 2) + "╯\r\n")
    emit("".join(rows))


def submit(text):
    if log_path:
        with open(log_path, "a") as f:
            f.write(json.dumps(text) + "\n")
    transcript.append(text)
    buf.clear()
    draw()


start = time.time()
emit("loading hooks...\r\n")
# Not ready: drop every byte that arrives.
while time.time() - start < READY_DELAY:
    r, _, _ = select.select([fd], [], [], 0.01)
    if r:
        os.read(fd, 4096)
draw()

pending = []  # (due_time, char)


def applier():
    while True:
        time.sleep(0.002)
        changed = False
        with lock:
            now = time.time()
            while pending and pending[0][0] <= now:
                buf.append(pending.pop(0)[1])
                changed = True
            if changed:
                draw()


threading.Thread(target=applier, daemon=True).start()

while True:
    data = os.read(fd, 4096)
    if not data:
        break
    for ch in data.decode("utf-8", "replace"):
        with lock:
            if ch == "\r":
                if swallow > 0:
                    swallow -= 1
                    continue
                submit("".join(buf))
            elif ch == "\x15":
                pending.clear()
                buf.clear()
                draw()
            elif ch == "\x7f":
                if buf:
                    buf.pop()
                    draw()
            elif ch >= " ":
                if PROCESS_DELAY:
                    last = pending[-1][0] if pending else time.time()
                    pending.append((max(last, time.time()) + PROCESS_DELAY, ch))
                else:
                    buf.append(ch)
                    draw()
