"""The sandbox network's egress, as the dev node role sets it: the role's own policy, rendered
from its defaults by Ansible, through docker_egress's template and fake iptables, with packets
walked through the result. The team gateway on the NAS is the only way out.

Run: python3 -I roles/endora_dev_node/tests/test_endora_dev_node.py   (needs ansible installed)
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROLE = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROLE, "..", "docker_egress", "tests"))
import test_docker_egress as egress  # noqa: E402

NAS = "192.168.1.10"
SANDBOX = "172.31.250.9"


def the_roles_policy(directory):
    """endora_dev_node_sandbox_policy from the role's defaults, with the NAS at NAS."""
    dest = os.path.join(directory, "policy.json")
    args = json.dumps({"content": "{{ endora_dev_node_sandbox_policy | to_json }}", "dest": dest})
    subprocess.run(
        ["ansible", "localhost", "-c", "local", "-i", "localhost,", "-m", "ansible.builtin.copy",
         "-a", args, "-e", "@" + os.path.join(ROLE, "defaults", "main.yml"),
         "-e", "endora_nas_addr=" + NAS],
        check=True, capture_output=True, text=True,
    )
    with open(dest) as handle:
        return json.load(handle)


class SandboxEgress(unittest.TestCase):
    def setUp(self):
        if not shutil.which("ansible"):
            self.skipTest("ansible is not installed")
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.policy = the_roles_policy(self.dir)
        self.fw = egress.Firewall(self.dir)
        result = self.fw.run(egress.render(self.policy, self.dir))
        self.assertEqual(result.returncode, 0, result.stderr)

    def tearDown(self):
        self.tmp.cleanup()

    def out(self, dest, port, proto="tcp"):
        return self.fw.verdict(SANDBOX, dest, port, proto)

    def test_the_policy_is_the_contracts(self):
        self.assertEqual(self.policy["subnet"], "172.31.250.0/24")
        self.assertEqual(self.policy["bridge"], "br-sandbox")
        self.assertIs(self.policy["allow_dns"], False)
        self.assertEqual(self.policy["allow_public_tcp_ports"], [])
        self.assertEqual(self.policy["allow_host_tcp_ports"], [])

    def test_the_nas_gateway_is_reachable(self):
        self.assertEqual(self.out(NAS, 8788), "PASS", "the team gateway")

    def test_nothing_else_on_the_nas_is(self):
        # 3128: the egress proxy, tabled (ADR 0110).
        for port in (22, 80, 443, 3128, 8787, 8790, 11434):
            self.assertEqual(self.out(NAS, port), "DROP", "NAS port %d" % port)

    def test_the_rest_of_the_lan_is_not(self):
        for dest in ("10.0.0.1", "10.0.0.5", "192.0.2.20", "169.254.169.254", "172.18.0.2"):
            for port in (8788, 443):
                self.assertEqual(self.out(dest, port), "DROP", "%s:%d" % (dest, port))

    def test_the_internet_is_not_even_for_dns(self):
        self.assertEqual(self.out("203.0.113.10", 443), "DROP")
        self.assertEqual(self.out("203.0.113.10", 80), "DROP")
        self.assertEqual(self.out("1.1.1.1", 53, "udp"), "DROP")
        self.assertEqual(self.out("1.1.1.1", 53), "DROP")

    def test_the_pi_itself_is_not(self):
        for port in (22, 53, 2375):
            self.assertEqual(
                self.fw.verdict(SANDBOX, "172.31.250.1", port, interface="br-sandbox"), "DROP"
            )


if __name__ == "__main__":
    unittest.main()
