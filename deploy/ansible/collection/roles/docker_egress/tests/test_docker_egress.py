"""Tests for the docker_egress role: render a policy with Ansible, run it against a fake
iptables, and walk packets through the result.

Run: python3 -I roles/docker_egress/tests/test_docker_egress.py   (needs ansible installed)
"""
import ipaddress
import json
import os
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROLE = os.path.dirname(HERE)
FAKE = os.path.join(HERE, "fake_iptables.py")

MAIL = {
    "name": "mailmirror",
    "bridge": "br-mailmirror",
    "subnet": "172.31.250.0/24",
    "allow_dns": True,
    "allow_public_tcp_ports": [443, 993],
}


def render(policy, directory):
    """Renders the policy file exactly as the role would, with Ansible's own templating."""
    dest = os.path.join(directory, policy["name"] + ".sh")
    variables = os.path.join(directory, "vars.json")
    with open(variables, "w") as handle:
        json.dump({"docker_egress_policy": policy}, handle)
    subprocess.run(
        ["ansible", "localhost", "-c", "local", "-i", "localhost,", "-m", "template",
         "-a", "src=%s dest=%s" % (os.path.join(ROLE, "templates", "policy.sh.j2"), dest),
         "-e", "@" + variables],
        check=True, capture_output=True, text=True,
    )
    return dest


class Firewall:
    def __init__(self, directory):
        self.state = os.path.join(directory, "iptables.json")
        self.bin = os.path.join(directory, "iptables")
        with open(self.bin, "w") as handle:
            handle.write('#!/bin/sh\nexec python3 -I "%s" "$@"\n' % FAKE)
        os.chmod(self.bin, 0o755)

    def run(self, script, mode="apply"):
        env = dict(os.environ, IPT=self.bin, IPT_STATE=self.state)
        return subprocess.run(["sh", script, mode], env=env, capture_output=True, text=True)

    def chains(self):
        with open(self.state) as handle:
            return json.load(handle)

    def verdict(self, source, dest, port, proto="tcp", interface=None):
        """Where a new connection from a container ends up: walks DOCKER-USER (forwarded)
        or INPUT (to the host) the way the kernel would for this subset of rules."""
        chains = self.chains()

        def matches(rule):
            pairs = dict(zip(rule[::2], rule[1::2]))
            if "-m" in pairs and pairs.get("--ctstate"):
                return False  # a new connection is not ESTABLISHED
            if "-s" in pairs and ipaddress.ip_address(source) not in ipaddress.ip_network(pairs["-s"]):
                return False
            if "-d" in pairs and ipaddress.ip_address(dest) not in ipaddress.ip_network(pairs["-d"]):
                return False
            if "-i" in pairs and pairs["-i"] != interface:
                return False
            if "-p" in pairs and pairs["-p"] != proto:
                return False
            if "--dport" in pairs and int(pairs["--dport"]) != port:
                return False
            return True

        def walk(chain):
            for rule in chains.get(chain, []):
                if not matches(rule):
                    continue
                target = rule[rule.index("-j") + 1]
                if target in ("DROP", "ACCEPT"):
                    return target
                if target == "RETURN":
                    return None
                if target in chains:
                    result = walk(target)
                    if result:
                        return result
                    continue
                return "PASS"  # handed to ufw / Docker's own chains
            return None

        start = "INPUT" if interface else "DOCKER-USER"
        return walk(start) or "PASS"


class EgressPolicy(unittest.TestCase):
    def setUp(self):
        if not shutil.which("ansible"):
            self.skipTest("ansible is not installed")
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.fw = Firewall(self.dir)

    def tearDown(self):
        self.tmp.cleanup()

    def applied(self, policy):
        script = render(policy, self.dir)
        result = self.fw.run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        return script

    def test_the_mail_network_reaches_the_internet_on_its_ports_and_nothing_else(self):
        self.applied(MAIL)
        out = lambda dest, port, proto="tcp": self.fw.verdict("172.31.250.5", dest, port, proto)
        self.assertEqual(out("203.0.113.10", 443), "PASS", "Google's API over HTTPS")
        self.assertEqual(out("198.51.100.41", 993), "PASS", "iCloud IMAP")
        self.assertEqual(out("1.1.1.1", 53, "udp"), "PASS", "DNS")
        self.assertEqual(out("203.0.113.10", 80), "DROP", "plain HTTP is not on its list")
        self.assertEqual(out("203.0.113.10", 22), "DROP")
        for lan in ("192.168.1.10", "10.0.0.1", "10.0.0.5", "172.18.0.2", "169.254.169.254"):
            self.assertEqual(out(lan, 443), "DROP", "%s must be unreachable" % lan)
            self.assertEqual(out(lan, 53, "udp"), "DROP", "DNS to %s must be unreachable" % lan)

    def test_the_host_itself_is_unreachable(self):
        self.applied(MAIL)
        self.assertEqual(
            self.fw.verdict("172.31.250.5", "172.31.250.1", 22, interface="br-mailmirror"), "DROP"
        )

    def test_other_containers_traffic_is_not_touched(self):
        self.applied(MAIL)
        self.assertEqual(self.fw.verdict("172.18.0.7", "192.168.1.10", 443), "PASS")
        self.assertEqual(self.fw.verdict("172.18.0.7", "172.18.0.1", 22, interface="docker0"), "PASS")

    def test_applying_twice_hooks_in_once(self):
        script = self.applied(MAIL)
        first = self.fw.chains()
        self.fw.run(script)
        self.assertEqual(self.fw.chains(), first)
        self.assertEqual(
            self.fw.chains()["DOCKER-USER"][0], ["-s", "172.31.250.0/24", "-j", "EG-MAILMIRROR"]
        )
        self.assertEqual(self.fw.chains()["INPUT"][0], ["-i", "br-mailmirror", "-j", "EG-MAILMIRROR-IN"])

    def test_remove_takes_everything_out(self):
        script = self.applied(MAIL)
        self.assertEqual(self.fw.run(script, "remove").returncode, 0)
        chains = self.fw.chains()
        self.assertNotIn("EG-MAILMIRROR", chains)
        self.assertNotIn("EG-MAILMIRROR-IN", chains)
        self.assertEqual(chains["DOCKER-USER"], [["-j", "RETURN"]])
        self.assertEqual(chains["INPUT"], [["-j", "ufw-before-input"]])

    def test_a_network_with_nothing_allowed_reaches_nothing(self):
        # Tier 0 of the sandbox: no DNS, no ports, no LAN.
        self.applied({"name": "tier0", "bridge": "br-tier0", "subnet": "172.31.251.0/24"})
        out = lambda dest, port, proto="tcp": self.fw.verdict("172.31.251.9", dest, port, proto)
        self.assertEqual(out("203.0.113.10", 443), "DROP")
        self.assertEqual(out("1.1.1.1", 53, "udp"), "DROP")
        self.assertEqual(out("192.168.1.10", 443), "DROP")

    def test_a_listed_lan_service_is_reachable_and_nothing_beside_it(self):
        # Tier 1 shape: one LAN service, no internet.
        self.applied({
            "name": "tier1", "bridge": "br-tier1", "subnet": "172.31.252.0/24",
            "allow_lan": [{"dest": "192.168.1.10", "port": 8443}],
        })
        out = lambda dest, port: self.fw.verdict("172.31.252.9", dest, port)
        self.assertEqual(out("192.168.1.10", 8443), "PASS")
        self.assertEqual(out("192.168.1.10", 22), "DROP")
        # Another host on the LAN, on the same port: not listed, so not reachable.
        self.assertEqual(out("10.0.0.5", 8443), "DROP")
        self.assertEqual(out("203.0.113.10", 443), "DROP")

    def test_a_listed_host_port_is_reachable_and_no_other(self):
        # Tier 2 shape: only the read-only proxy on the host.
        self.applied({
            "name": "tier2", "bridge": "br-tier2", "subnet": "172.31.253.0/24",
            "allow_host_tcp_ports": [8099],
        })
        to_host = lambda port: self.fw.verdict("172.31.253.9", "172.31.253.1", port, interface="br-tier2")
        self.assertEqual(to_host(8099), "ACCEPT")
        self.assertEqual(to_host(22), "DROP")

    def test_the_runner_applies_every_policy_and_reports_a_failure(self):
        policies = os.path.join(self.dir, "policies")
        os.makedirs(policies)
        render(MAIL, policies)
        with open(os.path.join(policies, "broken.sh"), "w") as handle:
            handle.write("exit 3\n")
        env = dict(os.environ, IPT=self.fw.bin, IPT_STATE=self.fw.state, DOCKER_EGRESS_DIR=policies)
        result = subprocess.run(["sh", os.path.join(ROLE, "files", "docker-egress")], env=env,
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, "a broken policy must fail the unit")
        self.assertIn("EG-MAILMIRROR", self.fw.chains(), "and must not stop the others")


if __name__ == "__main__":
    unittest.main(verbosity=1)
