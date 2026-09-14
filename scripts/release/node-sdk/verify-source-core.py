#!/usr/bin/env python3
"""拒绝 RC 制品误用已发布 Core，而非当前提交中的 Core。"""
import json
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[1]).resolve()
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--format-version', '1', '--locked'], cwd=root, text=True))
core = [package for package in metadata['packages'] if package['name'] == 'awiki-im-core']
if len(core) != 1 or core[0]['source'] is not None or Path(core[0]['manifest_path']).resolve() != root / 'crates/im-core/Cargo.toml':
    raise SystemExit('RC must compile the checked-out Core, not a registry or Git dependency')
print('RC Core source verified:', core[0]['manifest_path'])
