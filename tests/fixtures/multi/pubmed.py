#!/usr/bin/env python3
# Test double: emits one PubMed paper on `search`. Lets feed::load prove two
# distinct sources merge into one paper list.
import json, sys

req = json.loads(sys.stdin.read() or "{}")
op = req.get("op", "")

if op == "search":
    out = {"records": [
        {"kind": "paper", "source": "PubMed", "topic": "T", "title": "PubMed Paper",
         "link": "https://doi.org/10.1/x", "date_label": "2026-03-01",
         "ts": 200, "summary": "s", "grounding": "g"}
    ]}
else:
    out = {"records": []}

sys.stdout.write(json.dumps(out))
