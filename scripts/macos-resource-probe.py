"""Sample only a chosen app's coalition/descendants, including its out-of-process WebViews.

Usage: python3 scripts/macos-resource-probe.py --pid PID --seconds 30 --output /tmp/resources.json
Read-only diagnostic, not app code. RSS sums include shared pages; they are not physical footprint.
Coalition ABI: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info_private.h
The private, diagnostic-only ABI must return its expected size or this probe refuses incomplete data.
"""
import argparse
import ctypes
import json
import pathlib
import platform
import statistics
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--pid', type=int, required=True)
parser.add_argument('--seconds', type=int, default=30)
parser.add_argument('--output', type=pathlib.Path, required=True)
args = parser.parse_args()
if platform.system() != 'Darwin' or args.pid <= 0 or not 1 <= args.seconds <= 600:
    parser.error('Requires macOS, a positive app PID, and 1–600 seconds')
lib = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
lib.proc_pidinfo.restype = ctypes.c_int


def coalition(pid):
    # Two coalition IDs (resource, jetsam) followed by three reserved uint64 fields.
    data = (ctypes.c_uint64 * 5)()
    size = lib.proc_pidinfo(pid, 20, 0, data, ctypes.sizeof(data))
    return tuple(data[:2]) if size == ctypes.sizeof(data) else None


def snapshot():
    target = coalition(args.pid)
    if not target or not all(target) or (samples and tuple(samples[0]['coalitions']) != target):
        raise RuntimeError('Target exited or coalition metadata unavailable; refusing main-only measurement')
    rows = []
    for line in subprocess.check_output(['/bin/ps', '-axo', 'pid=,ppid=,rss=,%cpu=,comm='], text=True).splitlines():
        parts = line.split(None, 4)
        if len(parts) != 5:
            continue
        pid, parent, rss, cpu, name = parts
        rows.append({'pid': int(pid), 'parent': int(parent), 'rssMiB': int(rss) / 1024,
                     'cpu': float(cpu), 'process': pathlib.Path(name).name})
    owned = {args.pid}
    for row in rows:
        ids = coalition(row['pid'])
        if ids and (ids[0] == target[0] or ids[1] == target[1]):
            owned.add(row['pid'])
    for _ in range(len(rows)):
        previous = len(owned)
        owned.update(row['pid'] for row in rows if row['parent'] in owned)
        if len(owned) == previous:
            break
    selected = [row for row in rows if row['pid'] in owned]
    if not any('WebContent' in row['process'] for row in selected):
        raise RuntimeError('No app-owned WebContent found; refusing incomplete WebView measurement')
    return {'at': round(time.monotonic() - started, 3), 'coalitions': target,
            'rssMiB': sum(row['rssMiB'] for row in selected), 'cpu': sum(row['cpu'] for row in selected),
            'processes': selected}


started = time.monotonic()
samples = []
while time.monotonic() - started < args.seconds:
    samples.append(snapshot())
    time.sleep(min(1, max(0, args.seconds - (time.monotonic() - started))))
result = {'pid': args.pid, 'platform': platform.platform(), 'machine': platform.machine(),
          'elapsed': time.monotonic() - started, 'samples': samples,
          'summary': {'minRssMiB': min(s['rssMiB'] for s in samples),
                      'maxRssMiB': max(s['rssMiB'] for s in samples),
                      'medianRssMiB': statistics.median(s['rssMiB'] for s in samples),
                      'meanCpu': statistics.mean(s['cpu'] for s in samples)}}
args.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result['summary'], indent=2))
