#!/usr/bin/env python3
"""Launch time, by the "bounce" rule: from process spawn to the first frame after
which nothing is painting or loading. Runs on a copy of the vault, never focused.

  python3 tools/harness/bounce.py [vault] [runs]     default: lib_clean, 15
"""
import os, shutil, statistics, subprocess, sys, tempfile, time

repo = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
vault = sys.argv[1] if len(sys.argv) > 1 else os.path.join(repo, 'tools/harness/lib_clean')
runs = int(sys.argv[2]) if len(sys.argv) > 2 else 15
binary = os.environ.get('THESIO_BIN') or os.path.join(repo, 'target/release/thesio')

work = tempfile.mkdtemp(prefix='thesio-bounce-')
lib = os.path.join(work, 'lib')
shutil.copytree(vault, lib)
script = os.path.join(work, 'script')
open(script, 'w').write('wait 20000\n')
marks = {}
for run in range(runs):
    env = dict(os.environ, THESIO_HOME=lib, THESIO_SCRIPT=script, THESIO_SHOTS=work)
    env['THESIO_BOUNCE_T0'] = repr(time.time() * 1000)
    done = subprocess.run([binary], env=env, capture_output=True, text=True, timeout=30)
    for line in done.stderr.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[0] == 'bounce':
            marks.setdefault(parts[1], []).append(float(parts[2]))
shutil.rmtree(work)
print(f'{runs} runs, first dropped as cold')
for label, times in marks.items():
    warm = times[1:] or times
    print(f'{label:>14}  median {statistics.median(warm):7.1f} ms   min {min(warm):7.1f}   max {max(warm):7.1f}')
