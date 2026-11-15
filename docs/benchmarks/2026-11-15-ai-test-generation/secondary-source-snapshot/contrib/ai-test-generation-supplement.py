#!/usr/bin/env python3
"""Separate diagnostic retries, excluded from the six-case primary comparison."""
from __future__ import annotations

import argparse
import asyncio
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TEST = "handlers::ai_test::client::evaluation::ai_test_quality_evaluation"


def load(path: Path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    assert spec and spec.loader
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


async def call(binary: Path, output: Path, arm: str, selected: str) -> None:
    env = {**os.environ, "CARGO_MANIFEST_DIR":str(ROOT/'helix-term'),
        "HELIX_RUNTIME":str(ROOT/'runtime'), "HELIX_AI_TEST_EVAL_ARM":arm,
        "HELIX_AI_TEST_EVAL_OUTPUT":str(output), "HELIX_AI_TEST_EVAL_WORK":str(ROOT/'.superpowers/ai-test-eval/projects'),
        "HELIX_AI_TEST_EVAL_CASE":selected, "HELIX_AI_TEST_PYTHON":str(ROOT/'.superpowers/ai-test/venv/Scripts/python.exe'),
        "PYTHONDONTWRITEBYTECODE":"1"}
    with (output/f'{arm}.test.log').open('w',encoding='utf-8') as log:
        child = await asyncio.create_subprocess_exec(str(binary),'--ignored','--exact',TEST,'--nocapture','--test-threads=1',
            cwd=ROOT,env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,creationflags=0x08000000)
        try:
            code = await asyncio.wait_for(child.wait(),900)
            if code:
                raise RuntimeError(f'{arm} exited {code}; inspect log')
        finally:
            if child.returncode is None:
                child.terminate()
                await child.wait()
    for row in [json.loads(line) for line in (output/f'{arm}.jsonl').read_text(encoding='utf-8').splitlines()]:
        print(arm,row['case'],'generation',row['generation_ms'],'accepted',row['accepted'],'pass',(row['baseline']or{}).get('success'),'error',row['generation_error'],flush=True)


async def run(binary: Path, output: Path) -> None:
    config = tomllib.loads((Path(os.environ['APPDATA'])/'helix/config.toml').read_text(encoding='utf-8-sig'))['editor']['ai']['providers']
    local = dict(config['local-27b']); cloud = dict(config['deepseek'])
    support = load(ROOT/'contrib/ai-generation-eval.py')
    support.ensure_idle()
    for arm in ['27b-thinking-retry','deepseek-thinking-diagnostic']:
        if (output/f'{arm}.jsonl').exists():
            raise FileExistsError(f'Refusing to mix prior diagnostic observations: {arm}')
    manifest = {'excluded_from_primary':True,'local_retry_cases':['c_copy_prefix'],
        'reason':'First local copy request had a transport error; preserve it and test once separately.',
        'deepseek_thinking_cases':['c_parse_u16','cpp_merge_ranges','python_unique_by'],
        'deepseek_reason':'Investigate the three non-thinking schema/link/macro failures; not a full third arm.',
        'request_timeout_ms':120000,'output_budget_note':'Same production request builder; local has 4096 final + 2048 reasoning tokens; DeepSeek max_tokens=4096 total.'}
    async with support.model_server(Path(os.path.expandvars(local['server-path'])),Path(os.path.expandvars(local['model-path'])),
        local['model'],output/'27b-retry.server.log') as (address,ms):
        manifest['local_load_ms']=ms
        local.pop('model-path',None);local['endpoint']=address+'/v1/chat/completions';local['context-size']=16384
        (output/'27b-thinking-retry.provider.json').write_text(json.dumps(local,ensure_ascii=False,indent=2),encoding='utf-8')
        print('Local diagnostic retry ready, load',round(ms),'ms',flush=True)
        await call(binary,output,'27b-thinking-retry','c_copy_prefix')
    cloud['thinking']=True;cloud['context-size']=16384
    (output/'deepseek-thinking-diagnostic.provider.json').write_text(json.dumps(cloud,ensure_ascii=False,indent=2),encoding='utf-8')
    print('Running DeepSeek thinking diagnostics on three cases...',flush=True)
    await call(binary,output,'deepseek-thinking-diagnostic','c_parse_u16,cpp_merge_ranges,python_unique_by')
    (output/'supplement-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')


def main() -> None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-binary',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    asyncio.run(run(args.test_binary.resolve(),args.output.resolve()))


if __name__=='__main__':
    main()
