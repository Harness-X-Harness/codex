import unittest
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
from live_diagnostics import safe_stderr

class DiagnosticsTests(unittest.TestCase):
    def test_keep_error_without_credentials(self):
        result = safe_stderr(b'compile failed\nsecret-key Authorization: Bearer secret-token\nhttps://user:pass@example.test/', 'secret-key')
        self.assertIn('compile failed',result['text'])
        for secret in ('secret-key','secret-token','user:pass'):
            self.assertNotIn(secret,result['text'])
    def test_auth_forms_and_boundary(self):
        for sample in [b'Authorization: Basic BASE64_CANARY', b'Cookie: a=COOKIE_A; b=COOKIE_B', b'{"api_key":"JSON_SECRET_CANARY"}', b'https://e.test/?sig=SIGNED_CANARY&token=TOKEN_CANARY']:
            self.assertNotIn('CANARY', safe_stderr(sample, '')['text'])
        data = b'x'*65532+b'LONGSECRET'
        self.assertNotIn('LONG', safe_stderr(data,'LONGSECRET')['text'])
    def test_bound(self):
        result=safe_stderr(b'x'*65537,'')
        self.assertTrue(result['truncated'])
        self.assertEqual(result['captured_bytes'],0)

class WorkflowRetentionTests(unittest.TestCase):
    def test_early_failure_keeps_workflow_identity_without_package(self):
        root = Path(__file__).resolve().parents[2]
        for workflow in ("grok-live.yml", "grok.yml"):
            text = (root / ".github/workflows" / workflow).read_text()
            def script(name):
                step = text.split("      - name: " + name + "\n", 1)[1].split("      - ", 1)[0]
                return textwrap.dedent(step.split("        run: |\n", 1)[1])
            with tempfile.TemporaryDirectory() as directory:
                env = dict(os.environ, RUNNER_TEMP=directory, GITHUB_SHA="a"*40,
                           GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="1", JOB_STATUS="failure")
                for name in ("Initialize Live evidence before setup", "Finalize available Live evidence"):
                    subprocess.run(["bash", "-e", "-c", script(name)], env=env, check=True)
                manifest = json.loads((Path(directory)/"live-evidence/workflow.json").read_text())
                self.assertEqual(manifest["state"], "failure")
                self.assertEqual(manifest["harness_sha"], "a"*40)

    def test_stderr_file_is_sanitized(self):
        with tempfile.TemporaryDirectory() as directory:
            source, target = Path(directory)/"stderr", Path(directory)/"safe.json"
            source.write_text("backend error\nAuthorization: Basic SYNTHETIC_CANARY\n")
            subprocess.run(["python3", str(Path(__file__).with_name("live_diagnostics.py")),
                            "--stderr", str(source), "--output", str(target)], check=True)
            result = target.read_text()
            self.assertIn("backend error", result)
            self.assertNotIn("SYNTHETIC_CANARY", result)

if __name__=='__main__': unittest.main()
