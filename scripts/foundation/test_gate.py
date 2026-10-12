import unittest

from gate import classify, summarize


class GateTests(unittest.TestCase):
    def test_missing_test_cannot_pass(self):
        self.assertEqual(classify('unrelated output', 'checkpoint'), 'unexecuted')

    def test_only_exact_test_name_counts(self):
        self.assertEqual(classify('test checkpoint_extra ... ok', 'checkpoint'), 'unexecuted')

    def test_failure_is_not_an_expected_success(self):
        self.assertEqual(classify('test checkpoint ... FAILED', 'checkpoint'), 'failed')

    def test_ignored_is_not_passed(self):
        self.assertEqual(classify('test checkpoint ... ignored', 'checkpoint'), 'unexecuted')

    def test_pending_and_command_error_block_gate(self):
        self.assertFalse(summarize(['passed', 'unexecuted'], 0)['passed'])
        self.assertFalse(summarize(['passed'], 1)['passed'])
        self.assertFalse(summarize([], 0)['passed'])
        self.assertTrue(summarize(['passed'], 0)['passed'])


if __name__ == '__main__':
    unittest.main()
