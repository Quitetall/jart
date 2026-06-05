#!/usr/bin/env python3
# Test double for the HF adapter's repo_search / space_search ops. Dispatches
# on the request's `op` and emits one Repo- / Space-shaped record so feed::load
# can prove repos/spaces flow into Feed.repos / Feed.spaces.
import json, sys

req = json.loads(sys.stdin.read() or "{}")
op = req.get("op", "")

if op == "repo_search":
    out = {"records": [
        {"kind": "model", "name": "org/eeg-net",
         "link": "https://huggingface.co/models/org/eeg-net",
         "downloads": "1234", "likes": "56"}
    ]}
elif op == "space_search":
    out = {"records": [
        {"name": "org/eeg-demo",
         "link": "https://huggingface.co/spaces/org/eeg-demo",
         "likes": "7", "sdk": "gradio"}
    ]}
else:
    # paper_search etc.: empty (no papers from this fixture).
    out = {"records": []}

sys.stdout.write(json.dumps(out))
