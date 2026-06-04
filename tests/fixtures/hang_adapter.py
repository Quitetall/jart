#!/usr/bin/env python3
# Test double: never responds (hangs after reading stdin). Exercises the
# Rust-side per-adapter timeout + kill_on_drop reaping.
import sys, time
_ = sys.stdin.read()
while True:
    time.sleep(3600)
