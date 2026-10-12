#!/usr/bin/env python3
"""Controlled scratch child, never a real daemon. Exits itself after 8 seconds."""
import json, os, signal, socket, sys, threading, time
path = sys.argv[sys.argv.index('--socket') + 1]
root = os.path.dirname(path)
mode = open(os.path.join(root, 'mode')).read().strip()
log = os.path.join(root, 'requests')
lock = threading.Lock()
health_count = 0

def record(value):
    with lock:
        with open(log, 'a') as f:
            f.write(json.dumps(value) + '\n')

def on_signal(sig, frame):
    record({'signal': sig})
    os._exit(90)
signal.signal(signal.SIGTERM, on_signal)
signal.signal(signal.SIGINT, on_signal)
threading.Timer(8, lambda: os._exit(0)).start()

def serve(conn, replacement=False):
    global health_count
    with conn:
        f = conn.makefile('rwb', buffering=0)
        for line in f:
            mode = open(os.path.join(root, 'mode')).read().strip()
            req = json.loads(line)
            record({'connection': conn.fileno(), 'replacement': replacement, 'type': req['type']})
            if mode == 'mute':
                time.sleep(8)
                return
            if mode == 'busy':
                reply = {'type': 'error', 'code': 'BUSY', 'message': 'fixture busy'}
            elif req['type'] == 'health':
                health_count += 1
                reply = {'type': 'health_report', 'pid': (int(open(os.path.join(root, 'spoof-pid')).read()) if mode == 'spoof' else os.getpid() + (1 if replacement else 0)),
                         'uptime_seconds': 1, 'protocol_version': 7 if mode == 'old' else 8,
                         'projects': [], 'agent_count': 0}
                if mode == 'rebind' and health_count == 2 and not replacement:
                    os.unlink(path)
                    listener = socket.socket(socket.AF_UNIX)
                    listener.bind(path)
                    listener.listen()
                    threading.Thread(target=accept, args=(listener, True), daemon=True).start()
            elif req['type'] == 'suspend':
                reply = {'type': 'suspend_result', 'suspended': []}
            elif req['type'] == 'shutdown':
                reply = {'type': 'shutting_down'}
                f.write((json.dumps(reply) + '\n').encode())
                if mode != 'linger':
                    if mode == 'healthy':
                        os.unlink(path)
                    os._exit(0)
                continue
            else:
                reply = {'type': 'ok'}
            f.write((json.dumps(reply) + '\n').encode())

def accept(listener, replacement=False):
    while True:
        conn, _ = listener.accept()
        threading.Thread(target=serve, args=(conn, replacement), daemon=True).start()

listener = socket.socket(socket.AF_UNIX)
try:
    listener.bind(path)
except OSError:
    # Model a flock loser without unlinking the live endpoint.
    os._exit(0)
listener.listen()
accept(listener)
