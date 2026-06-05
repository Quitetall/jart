#!/usr/bin/env python3
# Test double: echoes one Repo-shaped record. Exercises the generic
# fetch_records::<Repo> decode path (it is not Paper-bound).
import json, sys
_ = sys.stdin.read()
sys.stdout.write(json.dumps({"records": [
    {"kind": "model", "name": "org/eeg-net",
     "link": "https://huggingface.co/models/org/eeg-net",
     "downloads": "1234", "likes": "56"}
]}))
