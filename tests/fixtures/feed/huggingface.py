#!/usr/bin/env python3
# Echo adapter aliased as "huggingface" for feed::load tests.
import json, sys
_ = sys.stdin.read()
sys.stdout.write(json.dumps({"records": [
    {"kind": "paper", "source": "HF", "topic": "T", "title": "Echo",
     "link": "https://example.com/x", "date_label": "2026-01-01",
     "ts": 1, "summary": "s", "grounding": "g"}
]}))
