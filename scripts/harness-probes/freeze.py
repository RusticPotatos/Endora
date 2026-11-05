#!/usr/bin/env python3
"""Freezes the task contracts together with the graders (PROTOCOL.md, "Freezing"): writes every family's
task words, allowed files and test-addition rule for the given seeds to `contracts-cN.json`, hashes it with
the rig's files, and prints the manifest and one combined hash. Python 3 standard library only.

    python3 freeze.py --contract 2 --seeds 0 1 2 3 --out DIR

Only exposed seeds (0-3) or seeds the researcher has picked for a confirmation after the freeze belong
here: the contracts file names the seeds it was made for.
"""

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import probes  # noqa: E402

FROZEN = ["probes.py", "controls.py", "accounting.py", "tariff.json", "SCHEMA.md", "PROTOCOL.md",
          "endora_local.py", "endora_live.py", "cc_local.py", "rig.py", "sidefx.py", "freeze.py"]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def contracts(contract, seeds):
    """The words and edit rules of every family at `contract`, for `seeds`."""
    out = {"contract": contract, "seeds": seeds, "protocol": probes.PROTOCOLS[-1], "tasks": []}
    for family in probes.FAMILIES:
        for seed in seeds:
            spec = probes.build(family, seed, contract)
            out["tasks"].append({
                "family": family, "seed": seed, "words": spec.words, "expected": spec.expected,
                "allowed": sorted(spec.allowed), "permitted": sorted(spec.data.get("permitted", ())),
                "test_additions_allowed": family in probes.TEST_ADDITIONS_ALLOWED})
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--contract", type=int, default=probes.LATEST_CONTRACT)
    p.add_argument("--seeds", type=int, nargs="+", required=True)
    p.add_argument("--out", required=True)
    a = p.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    name = f"contracts-c{a.contract}.json"
    (out / name).write_text(json.dumps(contracts(a.contract, a.seeds), indent=1, sort_keys=True) + "\n")
    lines = [f"{sha(HERE / f)}  {f}" for f in FROZEN] + [f"{sha(out / name)}  {name}"]
    manifest = "\n".join(lines) + "\n"
    combined = hashlib.sha256(manifest.encode()).hexdigest()
    (out / "MANIFEST.sha256").write_text(manifest + f"# combined {combined}\n")
    sys.stdout.write(manifest + f"combined {combined}\n")


if __name__ == "__main__":
    main()
