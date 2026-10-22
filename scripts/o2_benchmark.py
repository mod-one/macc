#!/usr/bin/env python3
"""Run short O2 experiments through registered MACC adapters (plan by default)."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'automat'))
from benchmarks.o2.cli import main

if __name__ == '__main__':
    raise SystemExit(main())
