"""Behavior tests for the private Ansible job agent."""
import importlib.util
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('jobs', pathlib.Path(__file__).resolve().parents[1] / 'files' / 'terminal-jobs.py')
jobs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(jobs)

class AgentTests(unittest.TestCase):
    def test_fixed_arguments_and_no_arbitrary_job_input(self):
        job = {'id': 'check-example', 'kind': 'check', 'role': 'homepage', 'label': 'Check dashboard'}
        command = jobs.arguments(job, '/private/example', '/private/example/inventory')
        self.assertIn('--check', command)
        self.assertNotIn('--diff', command)
        self.assertEqual(command[-2:], ['--tags', 'homepage'])
        with self.assertRaises(ValueError):
            jobs.arguments(dict(job, role='homepage; echo secret'), '/private/example', 'inventory')
        with self.assertRaises(ValueError):
            jobs.arguments(dict(job, kind='shell'), '/private/example', 'inventory')

    def test_durable_deduplication_and_restart_uncertainty(self):
        with tempfile.TemporaryDirectory() as folder:
            database = jobs.Database(pathlib.Path(folder) / 'jobs.db')
            first = database.submit('fictional-id', 'check-example')
            self.assertEqual(database.submit('fictional-id', 'check-example'), first)
            with self.assertRaises(ValueError):
                database.submit('fictional-id', 'apply-example')
            database.update('fictional-id', 'running', 'Started')
            restarted = jobs.Database(pathlib.Path(folder) / 'jobs.db')
            self.assertEqual(restarted.get('fictional-id')['state'], 'uncertain')
            self.assertEqual(restarted.next_job(), None)

    def test_private_log_output_is_bounded_and_credentials_redacted(self):
        text = jobs.redact('api_key=fictional-secret\npassword: fictional-password\nplain status')
        self.assertNotIn('fictional-secret', text)
        self.assertNotIn('fictional-password', text)
        self.assertIn('plain status', text)
        text = jobs.redact('{"apiKey": "fictional-json-key"}\nAuthorization: Bearer fictional-bearer')
        self.assertNotIn('fictional-json-key', text)
        self.assertNotIn('fictional-bearer', text)

    def test_log_retention_never_forgets_a_mutation_identifier(self):
        with tempfile.TemporaryDirectory() as folder:
            database = jobs.Database(pathlib.Path(folder) / 'jobs.db')
            database.submit('first-operation', 'check-example')
            database.update('first-operation', 'complete', 'Old output')
            for i in range(260):
                database.submit(f'operation-{i}', 'check-example')
                database.update(f'operation-{i}', 'complete', 'New output')
            replay = database.submit('first-operation', 'check-example')
            self.assertEqual(replay['state'], 'complete')
            self.assertEqual(database.next_job(), None)

if __name__ == '__main__':
    unittest.main()
