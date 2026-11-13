import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--report')
parser.add_argument('--reference', action='store_true')
parser.add_argument('--split', choices=['all', 'development', 'holdout'], default='all')
parser.add_argument('--fixtures', default=str(Path(__file__).with_name('ai-completion-eval-fixtures.json')))
args = parser.parse_args()
fixtures = json.loads(Path(args.fixtures).read_text(encoding='utf-8'))
if not args.reference and not args.report:
    parser.error('--report is required unless --reference is used')
report_rows = [] if args.reference else json.loads(Path(args.report).read_text(encoding='utf-8'))
report = {row['id']: row for row in report_rows}
if len(report) != len(report_rows): parser.error('Duplicate fixture ids in report')
required = {f['id'] for f in fixtures if args.split == 'all' or f['split'] == args.split}
if not args.reference and not required.issubset(report):
    parser.error('The report is missing requested fixture results')
if not args.reference and any('completion' not in report[key] for key in required):
    parser.error('Each result requires a completion field (string or null)')
if not args.reference and any(report[key]['completion'] is not None and not isinstance(report[key]['completion'],str) for key in required):
    parser.error('A completion must be a string or null')
header = '#include <algorithm>\n#include <cassert>\n#include <climits>\n#include <cstdint>\n#include <vector>\n#undef assert\n#define assert(expr) do { if (!(expr)) return 23; } while (0)\n'
passed = offered = total = withheld = request_errors = 0
rows = []
with tempfile.TemporaryDirectory(prefix='helix-ai-eval-', ignore_cleanup_errors=True) as temp:
    directory = Path(temp)
    for fixture in fixtures:
        if args.split != 'all' and fixture['split'] != args.split: continue
        total += 1
        row = report.get(fixture['id'], {})
        completion = fixture['reference'] if args.reference else row.get('completion')
        if not args.reference and row.get('error'):
            request_errors += 1
            result = 'request_error'
            error = row['error']
        elif completion is None:
            withheld += 1
            result = 'withheld'
            error = ''
        else:
            offered += 1
            program = fixture['code'].replace('<CURSOR>', completion)
            if fixture['language'] == 'python':
                path = directory/'case.py'
                path.write_text(program + '\n' + fixture['checks'], encoding='utf-8')
                command = [sys.executable, '-I', str(path)]
            else:
                path = directory/(fixture['id']+'.cpp')
                path.write_text(header+program+'\nint main(){'+fixture['checks']+'}\n', encoding='utf-8')
                compiler = shutil.which('clang++.exe') or shutil.which('clang++') or shutil.which('g++')
                if compiler is None: raise RuntimeError('C++ fixtures require clang++ or g++ on PATH')
                executable = directory/(fixture['id']+'.exe')
                built = subprocess.run([compiler,'-std=c++20','-O1','-fwrapv',str(path),'-o',str(executable)], capture_output=True, text=True, timeout=30)
                if built.returncode:
                    result='compile_error';error=built.stderr[-1000:]
                    rows.append(dict(id=fixture['id'], result=result, completion=completion, error=error))
                    print(f'{fixture["id"]}: {result}: {completion!r}', flush=True)
                    continue
                command = [str(executable)]
            try:
                ran = subprocess.run(command, capture_output=True, text=True, timeout=3)
                result = 'passed' if ran.returncode == 0 else 'wrong'
                error = ran.stderr[-1000:]
            except subprocess.TimeoutExpired:
                result='timeout';error='execution timeout'
        passed += result == 'passed'
        rows.append(dict(id=fixture['id'], result=result, completion=completion, error=error))
        print(f'{fixture["id"]}: {result}: {completion!r}', flush=True)
print(json.dumps(dict(total=total, offered=offered, passed=passed, wrong=offered-passed, withheld=withheld, request_errors=request_errors)), flush=True)
out = 'target/ai-semantic-reference-score.json' if args.reference else args.report.removesuffix('.json')+'-score.json'
Path(out).parent.mkdir(parents=True,exist_ok=True)
Path(out).write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding='utf-8')
if request_errors or (args.reference and passed != total): sys.exit(1)
