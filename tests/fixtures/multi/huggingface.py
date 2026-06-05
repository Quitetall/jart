#!/usr/bin/env python3
# Test double: emits one HF paper on paper_search, nothing on repo/space ops.
import json, sys

req = json.loads(sys.stdin.read() or "{}")
op = req.get("op", "")

if op == "paper_search":
    out = {"records": [
        {"kind": "paper", "source": "HF", "topic": "T", "title": "HF Paper",
         "link": "https://huggingface.co/papers/1", "date_label": "2026-02-01",
         "ts": 100, "summary": "s", "grounding": "g"}
    ]}
else:
    out = {"records": []}

sys.stdout.write(json.dumps(out))
