# mdiff-owned metadata bridge. Never sends transcript paths, prompts or tool payloads.
import json, socket, sys, time
try:
    payload = sys.stdin.buffer.read(65537)
    if len(payload) > 65536:
        raise ValueError('oversize')
    value = json.loads(payload)
    event = {key: value[key] for key in ('hook_event_name', 'session_id', 'agent_id', 'cwd', 'notification_type') if key in value}
    event['tool'] = sys.argv[2]
    event['sent'] = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
    data = json.dumps(event).encode()
    if len(data) > 8192:
        raise ValueError('oversize')
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(0.75)
        connection.connect(sys.argv[1])
        connection.sendall(data + b'\n')
        connection.recv(1)  # Stay alive until kernel peer/ancestry validation completes.
except (OSError, ValueError, IndexError, TypeError):
    pass
# No output/decision: hooks never grant permission, block a tool or add agent instructions.
