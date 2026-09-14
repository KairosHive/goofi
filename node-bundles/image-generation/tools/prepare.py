"""Prepare the optional FluxRT runtime in an explicitly selected Python environment."""
import argparse
import json
from pathlib import Path
import subprocess

REVISION = '32206e8255b085b9075e05d389781bcbc728d513'


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--python', required=True, help='goofi GIL interpreter, or a standalone test interpreter')
    parser.add_argument('--root', required=True, type=Path, help='FluxRT checkout path')
    options = parser.parse_args()
    packages = json.loads(subprocess.check_output([options.python, '-c',
        'import importlib.metadata as m, json; print(json.dumps([d.metadata["Name"].lower() for d in m.distributions()]))'], text=True))
    conflicts = set(packages) & {'opencv-python', 'opencv-python-headless', 'opencv-contrib-python-headless'}
    if conflicts:
        raise SystemExit(f'Use an environment with only opencv-contrib-python; conflicting packages: {sorted(conflicts)}')
    root = options.root.absolute()
    here = Path(__file__).absolute().parent
    if not root.exists():
        run('git', 'clone', 'https://github.com/tensorforger/FluxRT.git', root)
        run('git', '-C', root, 'checkout', '--detach', REVISION)
    revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != REVISION:
        raise SystemExit(f'Expected FluxRT {REVISION}; use a separate checkout for this runtime')
    patch = here / 'runtime.patch'
    applied = subprocess.run(['git', '-C', str(root), 'apply', '--unidiff-zero', '--reverse', '--check', str(patch)],
                             capture_output=True).returncode == 0
    if not applied:
        run('git', '-C', root, 'apply', '--unidiff-zero', '--check', patch)
        run('git', '-C', root, 'apply', '--unidiff-zero', patch)
    run('uv', 'pip', 'install', '--python', options.python, '-r', here / 'requirements.txt', '-e', root)
    run(options.python, here / 'download.py', root)


if __name__ == '__main__':
    main()
