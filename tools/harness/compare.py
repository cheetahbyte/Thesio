"""Screenshot regression: run one script with two builds and diff every shot.

usage: python3 tools/harness/compare.py <script.txt> <bin_a> <bin_b> [work_dir] [lib_dir]
  work_dir default: /tmp/thesio-compare   (a/, b/ and diff-<shot>.png land here)
Exit status 1 when any shot differs or is missing from either side, or when
there are no shots. Needs Pillow. A pure refactor should report every shot
identical. Scripts should park the pointer first, since the real mouse position
shows as hover.
"""
import os, shutil, subprocess, sys
from PIL import Image, ImageChops
here = os.path.dirname(os.path.abspath(__file__))
script, bins = sys.argv[1], sys.argv[2:4]
work = os.path.abspath(sys.argv[4]) if len(sys.argv) > 4 else '/tmp/thesio-compare'
lib = sys.argv[5] if len(sys.argv) > 5 else os.path.join(here, 'lib_clean')
for side, binary in zip('ab', bins):
    shutil.rmtree(os.path.join(work, side), ignore_errors=True)
    env = dict(os.environ, THESIO_BIN=os.path.abspath(binary))
    subprocess.run([sys.executable, os.path.join(here, 'run.py'), script, lib, os.path.join(work, side)], env=env, check=True)
pngs = lambda side: {f for f in os.listdir(os.path.join(work, side)) if f.endswith('.png')}
shots_a, shots_b = pngs('a'), pngs('b')
if not shots_a and not shots_b: sys.exit('no shots taken')
bad = 0
for name in os.listdir(work):
    if name.startswith('diff-'): os.remove(os.path.join(work, name))
for f in sorted(shots_a | shots_b):
    if f not in shots_a or f not in shots_b:
        print(f"{f}: missing in {'b' if f in shots_a else 'a'}"); bad += 1; continue
    pb = os.path.join(work, 'b', f)
    a, b = Image.open(os.path.join(work, 'a', f)).convert('RGB'), Image.open(pb).convert('RGB')
    if a.size != b.size:
        print(f'{f}: size {a.size} vs {b.size}'); bad += 1; continue
    d = ImageChops.difference(a, b)
    box = d.getbbox()
    if box is None:
        print(f'{f}: identical'); continue
    n = sum(1 for p in d.getdata() if p != (0, 0, 0))
    print(f'{f}: {n} pixels differ in {box}'); bad += 1
    d.point(lambda v: 255 if v else 0).save(os.path.join(work, f'diff-{f}'))
sys.exit(1 if bad else 0)
