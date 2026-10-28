"""One temporary experiment; no configurable model, upstream, or request budget."""
import argparse,hashlib,json,os,re,subprocess,tempfile,signal
from pathlib import Path
from relay import Relay
from offline_probe import sse
ROOT=Path(__file__).resolve().parent
RUNTIME_SHA='47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16'

def digest(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def source_identity():
 return {p.name:digest(p) for p in [ROOT/'driver.test',ROOT/'driver_test.go',ROOT/'wrapper.py',ROOT/'relay.py',ROOT/'run.py',ROOT/'offline_probe.py']}
def write(path,value):path.write_text(json.dumps(value,indent=2,sort_keys=True)+'\n');path.chmod(0o600)
def normalized(body):
 # Only normal fresh-session identity is normalized, never arbitrary input fields.
 body=json.loads(json.dumps(body));body.pop('prompt_cache_key',None)
 body.setdefault('reasoning',{}).pop('effort',None)
 for item in body.get('input',[]):
  if item.get('type')=='function_call_output':
   if not re.fullmatch(r'[0-9a-f]{64}',item.get('output','')):raise ValueError('offline_tool_token')
   item['output']='FRESH_RANDOM_TOKEN'
 def walk(v):
  if isinstance(v,str):return re.sub(r'/tmp/grok-live-[A-Za-z0-9_-]+','/tmp/grok-live-SESSION',v)
  if isinstance(v,list):return [walk(x) for x in v]
  if isinstance(v,dict):return {k:walk(x) for k,x in v.items()}
  return v
 return walk(body)
def run_arm(relay,arm,ordinal,binary,out,offline=False):
 relay.begin_arm(f'{ordinal}-{arm}',None if arm=='A' else 'high')
 result=out/f'arm-{ordinal}-{arm}.json';env=os.environ.copy()
 env.update(GROK_EFFORT_ARM=arm,GROK_EFFORT_BASE_URL=relay.url,GROK_EFFORT_WRAPPER=str(ROOT/'wrapper.py'),GROK_EFFORT_RESULT=str(result),GROK_EFFORT_REAL_BINARY=str(binary),GROK_EFFORT_REAL_SHA256=RUNTIME_SHA)
 if offline:env['GROK_API_KEY']='offline-fixture-key'
 try:
  p=subprocess.Popen([str(ROOT/'driver.test'),'-test.run=^TestEffortExperimentFirstTurn$','-test.count=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
  p.communicate(timeout=270)
  result_data=json.loads(result.read_text()) if result.exists() else {'passed':False,'failure':{'code':'driver_result_missing'}}
  result_data['driver_exit']=p.returncode
 except subprocess.TimeoutExpired:
  os.killpg(p.pid,signal.SIGKILL);p.communicate();result_data={'passed':False,'failure':{'code':'driver_timeout'}}
 relay.end_arm();write(result,result_data);return result_data

def main():
 parser=argparse.ArgumentParser();mode=parser.add_mutually_exclusive_group(required=True);mode.add_argument('--offline',action='store_true');mode.add_argument('--execute',action='store_true');parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--offline-proof',type=Path);args=parser.parse_args()
 binary=args.binary.resolve();out=args.output.resolve();out.mkdir(mode=0o700,parents=True,exist_ok=False);os.chmod(ROOT/'wrapper.py',0o755)
 if digest(binary)!=RUNTIME_SHA:raise ValueError('runtime_identity')
 identity=source_identity()
 if args.offline:
  seen=[]
  def scripted(body,headers):
   seen.append(body);return 200,{'Content-Type':'text/event-stream'},sse(body,len(seen))
  with Relay(mode='offline',offline_transport=scripted) as relay:
   seen.clear();a=run_arm(relay,'A',1,binary,out,True);a_req=list(seen)
   seen.clear();b=run_arm(relay,'B',2,binary,out,True);b_req=list(seen)
   if not a.get('passed') or not b.get('passed') or len(a_req)!=2 or len(b_req)!=2:raise ValueError('offline_roundtrip')
   if a_req[0].get('reasoning',{}).get('effort') is not None or b_req[0].get('reasoning',{}).get('effort')!='high':raise ValueError('offline_effort')
   if any(normalized(a)!=normalized(b) for a,b in zip(a_req,b_req)):raise ValueError('offline_non_effort_delta')
   for req in [a_req[1],b_req[1]]:
    calls=[x for x in req['input'] if x.get('type')=='function_call'];outputs=[x for x in req['input'] if x.get('type')=='function_call_output'];reasons=[x for x in req['input'] if x.get('type')=='reasoning']
    if len(calls)!=1 or len(outputs)!=1 or calls[0]['call_id']!=outputs[0]['call_id'] or not any(x.get('encrypted_content')=='opaque-offline-cipher' for x in reasons):raise ValueError('offline_continuation')
  denied=[]
  def deny(body,headers):denied.append(1);return 403,{'Content-Type':'application/json'},b'{"code":"fixture_denied"}'
  with Relay(mode='offline',offline_transport=deny) as relay:
   result=run_arm(relay,'A',3,binary,out,True)
   if len(denied)!=1 or result.get('passed') or result.get('failure',{}).get('http_status')!=403:raise ValueError('offline_zero_retry')
  write(out/'proof.json',{'passed':True,'runtime_sha256':RUNTIME_SHA,'source_files':identity,'request_delta':'effort only after fresh-session cache key, temporary cwd and fresh tool token normalization','loopback_roundtrip_requests':4,'loopback_denial_requests':1,'real_http_requests':0});return
 proof=json.loads(args.offline_proof.read_text()) if args.offline_proof else {}
 if proof.get('passed') is not True or proof.get('source_files')!=identity or proof.get('runtime_sha256')!=RUNTIME_SHA:raise ValueError('offline_proof_binding')
 if not os.environ.get('GROK_API_KEY'):raise ValueError('credential_unavailable')
 results=[]
 with Relay(mode='real') as relay:
  try:
   def clean(r):return not r.get('cleanup_pending',False) and (r.get('passed') is True or r.get('failure',{}).get('http_status')==403)
   for arm in ['A','B']:
    results.append(run_arm(relay,arm,len(results)+1,binary,out))
    if not clean(results[-1]):break
   if len(results)==2 and all(clean(r) for r in results) and bool(results[0].get('passed')) != bool(results[1].get('passed')):
    for arm in ['B','A']:
     results.append(run_arm(relay,arm,len(results)+1,binary,out))
     if not clean(results[-1]):break
  finally:write(out/'relay.json',relay.summary())
 write(out/'summary.json',{'runtime_sha256':RUNTIME_SHA,'artifact_id':12769806175,'model':'grok-4.6','results':results,'source_files':identity,'causal_conclusion':'exploratory only; no production oracle changed'})
if __name__=='__main__':
 try:main()
 except Exception as exc:
  safe_codes={'runtime_identity','offline_roundtrip','offline_effort','offline_non_effort_delta','offline_zero_retry','offline_tool_token','offline_continuation','offline_proof_binding','credential_unavailable'}
  code=str(exc) if type(exc) is ValueError and str(exc) in safe_codes else 'experiment_failure'
  print('bounded experiment stopped: '+code);raise SystemExit(1)
