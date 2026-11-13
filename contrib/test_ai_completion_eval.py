"""CLI regressions for distinguishing missing/error/withheld benchmark results."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ai-completion-eval.py')

class EvaluationReportTests(unittest.TestCase):
    def run_report(self, report):
        with tempfile.TemporaryDirectory(prefix='helix-ai-report-test-') as temp:
            directory = Path(temp)
            fixtures = directory/'fixtures.json'
            fixtures.write_text(json.dumps([dict(id='sample',language='python',code='def answer():\n    return <CURSOR>\n',reference='42',checks='assert answer()==42',split='development')]),encoding='utf-8')
            path = directory/'report.json'
            path.write_text(json.dumps(report),encoding='utf-8')
            run = subprocess.run([sys.executable,str(SCRIPT),'--fixtures',str(fixtures),'--report',str(path)],capture_output=True,text=True,timeout=10)
            scored = path.with_name('report-score.json')
            return run, json.loads(scored.read_text(encoding='utf-8')) if scored.exists() else None

    def test_missing_completion_is_not_abstention(self):
        result, rows = self.run_report([dict(id='sample')])
        self.assertNotEqual(result.returncode,0)
        self.assertIn('completion field',result.stderr)
        self.assertIsNone(rows)

    def test_backend_error_remains_an_error(self):
        result, rows = self.run_report([dict(id='sample',completion=None,error='AI request timed out')])
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(rows[0]['result'],'request_error')
        self.assertEqual(rows[0]['error'],'AI request timed out')
        self.assertEqual(json.loads(result.stdout.splitlines()[-1])['withheld'],0)

    def test_abstention_and_wrong_code_are_separate(self):
        result, rows = self.run_report([dict(id='sample',completion=None)])
        self.assertEqual(result.returncode,0)
        self.assertEqual(rows[0]['result'],'withheld')
        result, rows = self.run_report([dict(id='sample',completion='41')])
        self.assertEqual(result.returncode,0)
        self.assertEqual(rows[0]['result'],'wrong')

if __name__ == '__main__': unittest.main()
