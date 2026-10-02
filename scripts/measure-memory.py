#!/usr/bin/env python3
"""Sum RSS, PSS and private memory for agent/UI process trees from smaps_rollup."""
import os
import sys
from pathlib import Path

roots = {int(value) for value in sys.argv[1:]}
if not roots:
    raise SystemExit("Uso: measure-memory.py PID [PID ...]")
processes = {}
for entry in Path('/proc').iterdir():
    if not entry.name.isdigit():
        continue
    try:
        status = (entry / 'status').read_text()
        parent = int(next(line.split()[1] for line in status.splitlines() if line.startswith('PPid:')))
        processes[int(entry.name)] = parent
    except (OSError, StopIteration, ValueError):
        pass
selected = set(roots)
while True:
    added = {pid for pid, parent in processes.items() if parent in selected}
    if added <= selected:
        break
    selected |= added
rows = []
for pid in sorted(selected):
    try:
        rollup = (Path('/proc') / str(pid) / 'smaps_rollup').read_text()
        values = {parts[0].rstrip(':'): int(parts[1]) for line in rollup.splitlines() if len(parts := line.split()) >= 2 and parts[1].isdigit()}
        name = (Path('/proc') / str(pid) / 'comm').read_text().strip()
        rows.append((pid, name, values['Rss'], values['Pss'], values.get('Private_Clean', 0) + values.get('Private_Dirty', 0)))
    except (OSError, KeyError):
        pass
print('PID NAME RSS_KiB PSS_KiB PRIVATE_KiB')
for row in rows:
    print(*row)
print('TOTAL', '', *(sum(row[index] for row in rows) for index in (2, 3, 4)))
