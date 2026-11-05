"""Scripted run of Thesio against a throwaway library.

usage: python3 tools/harness/run.py <script.txt> [lib_dir] [work_dir]
  lib_dir  default: tools/harness/lib_clean  (copied fresh every run, never mutated)
  work_dir default: /tmp/thesio-run             (shots/*.png and `log` land here)
  THESIO_BIN  default: target/release/thesio of this checkout
Bench numbers: grep -a bench <work_dir>/log
"""
import os, shutil, subprocess, sys
here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.dirname(os.path.dirname(here))
lib_src = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else os.path.join(here, 'lib_clean')
work = os.path.abspath(sys.argv[3]) if len(sys.argv) > 3 else '/tmp/thesio-run'
os.makedirs(work, exist_ok=True)
script = os.path.join(work, 's.txt')
open(script, 'w').write(open(sys.argv[1]).read() + '\nquit\n')
lib = os.path.join(work, 'lib')
shutil.rmtree(lib, ignore_errors=True); shutil.copytree(lib_src, lib)
binary = os.path.abspath(os.environ.get('THESIO_BIN') or os.path.join(repo, 'target', 'release', 'thesio'))
env = {'HOME': os.path.expanduser('~'), 'THESIO_HOME': lib, 'THESIO_SCRIPT': script,
       'THESIO_SHOTS': work, 'PATH': '/opt/homebrew/bin:/usr/bin:/bin'}
for k in ('THESIO_TRACE', 'THESIO_NOPARK', 'THESIO_WINDOW_CAPTURE', 'THESIO_RECORD_FPS'):
    if k in os.environ: env[k] = os.environ[k]
sys.exit(subprocess.run(binary, env=env, cwd=work, stdout=open(os.path.join(work, 'log'), 'w'), stderr=subprocess.STDOUT).returncode)
