#!/usr/bin/env python3
# Test double: echo a fixed records payload regardless of input.
import json, sys
_ = sys.stdin.read()
sys.stdout.write(json.dumps({"records": [
    {"kind": "paper", "source": "HF", "topic": "t", "title": "Echo",
     "link": "https://example.com/x", "date_label": "2026-01-01",
     "ts": 1, "summary": "s", "grounding": "g"}
]}))
