#!/usr/bin/env python3
from datetime import datetime, timezone
from pathlib import Path
import re

ROOT = Path("/home/refcell/dev/base-perf-autopilot")
JOURNAL = ROOT / "docs/autonomy/perf-journal.md"


def last_entry_heading(text: str) -> str:
    matches = re.findall(r"^##\s+(.+)$", text, flags=re.MULTILINE)
    return matches[-1] if matches else "none"


text = JOURNAL.read_text() if JOURNAL.exists() else ""
now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
print(f"timestamp: {now}")
print(f"repo_root: {ROOT}")
print(f"journal: {JOURNAL}")
print(f"last_journal_entry: {last_entry_heading(text)}")
