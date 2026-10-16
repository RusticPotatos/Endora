#!/usr/bin/env python3
"""A stand-in for iptables that keeps chains and rules in a JSON file (IPT_STATE), for the
subset of commands the egress policies use: -N, -F, -X, -A, -I <chain> 1, -D."""
import json
import os
import sys

state_path = os.environ["IPT_STATE"]
try:
    with open(state_path) as handle:
        chains = json.load(handle)
except FileNotFoundError:
    chains = {"DOCKER-USER": [["-j", "RETURN"]], "INPUT": [["-j", "ufw-before-input"]]}


def save():
    with open(state_path, "w") as handle:
        json.dump(chains, handle)


args = sys.argv[1:]
op, chain, rest = args[0], args[1], args[2:]
if op == "-N":
    if chain in chains:
        sys.exit(1)
    chains[chain] = []
elif op == "-F":
    if chain not in chains:
        sys.exit(1)
    chains[chain] = []
elif op == "-X":
    if chain not in chains or chains[chain]:
        sys.exit(1)
    del chains[chain]
elif op == "-A":
    chains[chain].append(rest)
elif op == "-I":
    assert rest[0] == "1", rest
    chains[chain].insert(0, rest[1:])
elif op == "-D":
    if chain not in chains or rest not in chains[chain]:
        sys.exit(1)
    chains[chain].remove(rest)
else:
    sys.exit("unsupported: %s" % args)
save()
