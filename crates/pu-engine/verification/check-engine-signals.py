#!/usr/bin/env python3
"""Public-only Engine QA. Reuses the exact local fake-provider class from CLI probes."""
import argparse, ast, base64, hashlib, http.server, json, os, signal, socket, subprocess, tempfile, threading, time, urllib.request
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('tool',choices=['claude','codex','opencode'])
parser.add_argument('scenario',choices=['question','permission','error','interrupt'])
parser.add_argument('cli')
parser.add_argument('--runner',required=True,help='Exact candidate owned-native-signals.rs executable; compile against cargo compiler-artifact paths')
args=parser.parse_args();mode=args.scenario
runner_path=Path(args.runner).resolve()
provenance=json.loads((runner_path.parent/'provenance.json').read_text())
assert provenance['runner_sha256']==hashlib.sha256(runner_path.read_bytes()).hexdigest(), 'Runner differs from recorded compiler artifacts'
assert provenance['source_sha']==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(), 'Build the runner at this candidate'
assert provenance['source_diff_sha256']==hashlib.sha256(subprocess.check_output(['git','diff','HEAD','--','crates'])).hexdigest(), 'Source changed after runner build'

root=Path(tempfile.mkdtemp(prefix='ppqa-',dir='/tmp')).resolve();home=root/'home';config=root/'config';repo=root/'project';state=root/'state';data=root/'data'
for directory in [home,config,repo,state,data,root/'tmp',root/'cache']:directory.mkdir()
helper=Path.cwd()/f'crates/pu-engine/verification/check-{args.tool}-signals.py'
node=next(n for n in ast.parse(helper.read_text()).body if isinstance(n,ast.ClassDef) and n.name=='Handler')
exec(compile(ast.Module(body=[node],type_ignores=[]),str(helper),'exec'),globals())
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
env={'HOME':str(home),'TMPDIR':str(root/'tmp'),'PATH':'/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin','SHELL':'/bin/sh','TERM':'xterm-256color','LANG':'en_US.UTF-8'}
if args.tool=='claude':
 seed={'hasCompletedOnboarding':True,'customApiKeyResponses':{'approved':['sandbox-fixture'],'rejected':[]},'projects':{str(repo):{'hasTrustDialogAccepted':True,'hasCompletedProjectOnboarding':True}}}
 for path in [home/'.claude.json',config/'.claude.json']:path.write_text(json.dumps(seed))
 env.update({'CLAUDE_CONFIG_DIR':str(config),'ANTHROPIC_API_KEY':'sandbox-fixture','ANTHROPIC_BASE_URL':f'http://127.0.0.1:{server.server_port}','CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC':'1','DISABLE_AUTOUPDATER':'1'})
elif args.tool=='codex':
 (config/'config.toml').write_text(f'''model = "gpt-5.4"
model_provider = "fixture"
approval_policy = "on-request"
sandbox_mode = "read-only"
[model_providers.fixture]
name = "Isolated fixture"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
[projects."{repo}"]
trust_level = "trusted"
''')
 env['CODEX_HOME']=str(config)
else:
 settings={'model':'fixture/chat','update':'disable','share':'disabled','permissions':[{'action':'*','resource':'*','effect':'allow'},{'action':'edit','resource':'*','effect':'ask'}],'providers':{'fixture':{'name':'Fixture','package':'@opencode/ai/providers/openai-compatible','settings':{'apiKey':'sandbox-fixture','baseURL':f'http://127.0.0.1:{server.server_port}/v1'},'models':{'chat':{'name':'Native fixture','limit':{'context':32000,'output':1000},'capabilities':{'tools':True}}}}}}
 env.update({'XDG_STATE_HOME':str(state),'XDG_CONFIG_HOME':str(config),'XDG_DATA_HOME':str(data),'XDG_CACHE_HOME':str(root/'cache'),'OPENCODE_DISABLE_AUTOUPDATE':'1','OPENCODE_DISABLE_MODELS_FETCH':'1','OPENCODE_CONFIG_CONTENT':json.dumps(settings)})
 with socket.socket() as reservation:reservation.bind(('127.0.0.1',0));port=reservation.getsockname()[1]
 (config/'opencode').mkdir();path=config/'opencode/service.json';path.write_text(json.dumps({'hostname':'127.0.0.1','port':port}));path.chmod(0o600)
print('ROOT',root,'TOOL',args.tool,'SCENARIO',args.scenario,flush=True)
(root/'runner-provenance.json').write_text(json.dumps(provenance,indent=2))
(root/'fixture.json').write_text(json.dumps({'tool':args.tool,'scenario':args.scenario,'provider_source':str(helper),'worktree_source':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'runner':str(Path(args.runner).resolve())}))
log=open(root/'runner.log','w');process=None
try:
 process=subprocess.Popen([args.runner,str(root),args.tool,args.cli,args.scenario],env=env,stdout=log,stderr=subprocess.STDOUT)
 code=process.wait(timeout=100);log.close()
 print('RUNNER_EXIT',code,flush=True)
 print((root/'runner.log').read_text()[-3000:],flush=True)
 assert code==0,root
 pid_path=root/'owned-pid.txt'
 if pid_path.exists():
  pid=int(pid_path.read_text());deadline=time.monotonic()+3
  while True:
   try:os.kill(pid,0)
   except ProcessLookupError:break
   if time.monotonic()>deadline:raise AssertionError(f'owned PID {pid} still alive after cleanup')
   time.sleep(.1)
 print('OWNED_CLI_CLEANUP_PASS',flush=True)
finally:
 if process is not None and process.poll() is None:process.terminate();process.wait(timeout=3)
 log.close()
 registration=state/'opencode/service.json'
 if args.tool=='opencode' and registration.exists():
  record=json.loads(registration.read_text())
  try:
   header='Basic '+base64.b64encode(('opencode:'+record.get('password','')).encode()).decode()
   request=urllib.request.Request(record['url'].rstrip('/')+'/api/info',headers={'Authorization':header})
   health=json.load(urllib.request.urlopen(request,timeout=1))
   if health.get('pid')==record['pid'] and health.get('version')=='2.0.28':os.kill(record['pid'],signal.SIGTERM)
  except (OSError,ValueError):pass
 server.shutdown()
