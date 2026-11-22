"""Create a disposable wide history and measure Git traversal (not native app startup).

python3 scripts/history-probe.py /tmp/mdiff-history --create --commits 50000
Existing repositories are read only. --create refuses an existing destination.
"""
import argparse
import json
import pathlib
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('path', type=pathlib.Path)
parser.add_argument('--create', action='store_true')
parser.add_argument('--commits', type=int, default=50000)
args = parser.parse_args()
if args.create:
    if not 1 <= args.commits <= 200000:
        parser.error('--commits must be between 1 and 200000')
    args.path.mkdir(parents=True, exist_ok=False)
    subprocess.run(['git', 'init', '-q', '-b', 'main', str(args.path)], check=True)
    records = ['feature done', 'blob', 'mark :1', 'data 0', '',
               'commit refs/heads/main', 'mark :2',
               'author Probe <probe@example.com> 1700000000 +0000',
               'committer Probe <probe@example.com> 1700000000 +0000',
               'data 4', 'root', 'M 100644 :1 file.txt', '']
    tips = {'main': 2}
    for i in range(1, args.commits + 1):
        branch = 'main' if i % 129 == 0 else f'branch-{i % 129}'
        records += [f'commit refs/heads/{branch}', f'mark :{i+2}',
                    f'author Probe <probe@example.com> {1700000000+i} +0000',
                    f'committer Probe <probe@example.com> {1700000000+i} +0000',
                    'data 8', f'{i:08}', f'from :{tips.get(branch, 2)}']
        if branch == 'main':
            records += [f'merge :{tips.get(f"branch-{(i//129)%128+1}", 2)}']
        records += ['']
        tips[branch] = i + 2
    subprocess.run(['git', 'fast-import', '--quiet'], cwd=args.path,
                   input=('\n'.join(records) + '\ndone\n').encode(), check=True)
results = []
for order in ['--topo-order', '--date-order']:
    for _ in range(3):
        start = time.monotonic()
        process = subprocess.Popen(['git', 'log', order, '--branches', '--tags', '--remotes',
                                    'HEAD', '--format=%H %P %at %s'], cwd=args.path,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                process.stdout.readline()
            first = time.monotonic() - start
            for _ in range(10000):
                process.stdout.readline()
            results.append({'order': order, 'first100_ms': round(first*1000, 2),
                            'through10100_ms': round((time.monotonic()-start)*1000, 2)})
        finally:
            process.terminate()
            process.communicate()
print(json.dumps(results, indent=2))
