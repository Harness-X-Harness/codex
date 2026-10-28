"""One temporary header control; no configurable model, upstream, or request budget."""
import argparse,hashlib,json,os,re,subprocess,tempfile,signal
from pathlib import Path
from relay import Relay
from offline_probe import sse
ROOT=Path(__file__).resolve().parent
RUNTIME_SHA='47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16'

PROMPT_A='Call grok_history_probe, reason about its returned token, then return JSON with the exact token as the token field, or null if unavailable.'

def digest(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def source_identity():
 return {p.name:digest(p) for p in [ROOT/'driver.test',ROOT/'driver_test.go',ROOT/'wrapper.py',ROOT/'relay.py',ROOT/'run.py',ROOT/'offline_probe.py']}
def write(path,value):path.write_text(json.dumps(value,indent=2,sort_keys=True)+'\n');path.chmod(0o600)
def normalized(body,item_ids=None):
 # Only normal fresh-session identity is normalized, never arbitrary input fields.
 body=json.loads(json.dumps(body));body.pop('prompt_cache_key',None)
 if item_ids is None:item_ids={}
 for item in body.get('input',[]):
  # Runtime-created item identities, not tool call_id or semantic payload.
  prefix={'message':'msg','function_call_output':'fco'}.get(item.get('type'))
  value=item.get('id')
  if prefix and isinstance(value,str) and re.fullmatch(prefix+r'_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}',value):
   if value not in item_ids:item_ids[value]=prefix+'_SESSION_ITEM_'+str(len(item_ids))
   item['id']=item_ids[value]
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
 relay.begin_arm(f'{ordinal}-{arm}','high')
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

def value_summary(value):
 data=json.dumps(value,sort_keys=True,separators=(',',':')).encode()
 summary={'type':type(value).__name__,'json_bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
 if isinstance(value,(str,list,dict)):summary['length']=len(value)
 if isinstance(value,str):
  summary['known_markers']=[x for x in ['grok-live-','<cwd>','<current_date>','<timezone>','<session_id>','<thread_id>','<current_time_reminder>'] if x in value]
 return summary

def difference_report(a,b,path=''):
 differences=[]
 def walk(a,b,path):
  if type(a) is type(b) and isinstance(a,dict):
   for key in sorted(set(a)|set(b)):
    pointer=path+'/'+str(key).replace('~','~0').replace('/','~1')
    if key not in a or key not in b:
     differences.append({'path':pointer,'left_present':key in a,'right_present':key in b})
    else:walk(a[key],b[key],pointer)
  elif type(a) is type(b) and isinstance(a,list) and len(a)==len(b):
   for index,(left,right) in enumerate(zip(a,b)):walk(left,right,path+'/'+str(index))
  elif a!=b or type(a) is not type(b):
   entry={'path':path,'left':value_summary(a),'right':value_summary(b)}
   if isinstance(a,str) and isinstance(b,str):
    prefix=0
    while prefix<min(len(a),len(b)) and a[prefix]==b[prefix]:prefix+=1
    suffix=0
    while suffix<min(len(a),len(b))-prefix and a[len(a)-1-suffix]==b[len(b)-1-suffix]:suffix+=1
    entry['common_prefix_chars']=prefix;entry['common_suffix_chars']=suffix
    entry['changed_chars']=[len(a)-prefix-suffix,len(b)-prefix-suffix]
   differences.append(entry)
 walk(a,b,path)
 return {'count':len(differences),'differences':differences[:128],'truncated':len(differences)>128}

def offline_gate(binary,out,identity):
 report={'passed':False,'real_http_requests':0,'arms':{},'checks':{},'source_files':identity,'runtime_sha256':RUNTIME_SHA,'temporary_directory_is_tmp':tempfile.gettempdir()=='/tmp'}
 requests={}
 try:
  # Independent loopback arms/checks all run before aggregation. No real transport.
  for ordinal,arm in enumerate(['B','A'],1):
   seen=[]
   def scripted(body,headers):
    seen.append(body);return 200,{'Content-Type':'text/event-stream'},sse(body,len(seen))
   with Relay(mode='offline',offline_transport=scripted) as relay:
    try:report['arms'][arm]=run_arm(relay,arm,ordinal,binary,out,True)
    except Exception:report['arms'][arm]={'passed':False,'failure':{'code':'offline_arm_exception'}}
    finally:report['arms'][arm]['relay']=relay.summary();requests[arm]=list(seen)
  denied=[]
  def deny(body,headers):denied.append(1);return 403,{'Content-Type':'application/json'},b'{"code":"fixture_denied"}'
  with Relay(mode='offline',offline_transport=deny) as relay:
   try:denial=run_arm(relay,'A',3,binary,out,True)
   except Exception:denial={'passed':False,'failure':{'code':'offline_denial_exception'}}
   finally:report['denial']={'result':denial,'requests':len(denied),'relay':relay.summary()}
  checks=report['checks']
  checks['roundtrips']=all(report['arms'][arm].get('passed') and len(requests[arm])==2 for arm in ['A','B'])
  checks['cleanup']=all(not report['arms'][arm].get('cleanup_pending',True) for arm in ['A','B']) and not denial.get('cleanup_pending',True)
  checks['zero_retry']=len(denied)==1 and not denial.get('passed') and denial.get('failure',{}).get('http_status')==403
  report['comparisons']=[];continuations=[];efforts=[];prompts=[]
  item_ids={'A':{},'B':{}}
  for index in range(min(len(requests['A']),len(requests['B']))):
   a,b=requests['A'][index],requests['B'][index]
   comparison={'request':index+1,'raw':difference_report(a,b)}
   try:comparison['normalized']=difference_report(normalized(a,item_ids['A']),normalized(b,item_ids['B']))
   except Exception:comparison['normalization_error']='unexpected_tool_token_shape'
   report['comparisons'].append(comparison)
  for arm in ['A','B']:
   for req in requests[arm]:
    efforts.append(req.get('reasoning',{}).get('effort')=='high')
    expected=PROMPT_A
    prompts.append(sum(part.get('type')=='input_text' and part.get('text')==expected for item in req.get('input',[]) if item.get('type')=='message' and item.get('role')=='user' for part in item.get('content',[]))==1)
   if len(requests[arm])==2:
    req=requests[arm][1];calls=[x for x in req['input'] if x.get('type')=='function_call'];outputs=[x for x in req['input'] if x.get('type')=='function_call_output'];reasons=[x for x in req['input'] if x.get('type')=='reasoning']
    continuations.append(len(calls)==1 and len(outputs)==1 and calls[0]['call_id']==outputs[0]['call_id'] and any(x.get('encrypted_content')=='opaque-offline-cipher' for x in reasons))
  checks['profile_effort']=len(efforts)==4 and all(efforts)
  checks['prompt_binding']=len(prompts)==4 and all(prompts)
  checks['continuations']=len(continuations)==2 and all(continuations)
  checks['same_schema']=len(requests['A'])==2 and len(requests['B'])==2 and all(a.get('text')==b.get('text') for a,b in zip(requests['A'],requests['B']))
  checks['profile_binding']=all(report['arms'][arm].get('profile')=='shipped' and report['arms'][arm].get('shipped_catalog') is True for arm in ['A','B'])
  checks['same_body']=len(report['comparisons'])==2 and all(c.get('normalized',{}).get('count')==0 for c in report['comparisons'])
  native_headers={'x-codex-beta-features','x-codex-turn-metadata','x-codex-window-id'}
  checks['header_policy']=all(len(report['arms'][arm]['relay']['events'])==2 and all(native_headers.issubset(set(e.get('headers',{}).get('incoming',[]))) and native_headers.issubset(set(e.get('headers',{}).get('dropped' if arm=='A' else 'forwarded',[]))) for e in report['arms'][arm]['relay']['events']) for arm in ['A','B'])
  report['passed']=all(checks.values())
 except Exception:report['capture_failure']='offline_capture_exception'
 finally:write(out/'offline-report.json',report)
 if not report['passed']:raise ValueError('offline_gate_failed')
 write(out/'proof.json',{'passed':True,'runtime_sha256':RUNTIME_SHA,'source_files':identity,'request_delta':'header control: same normalized body/profile, only three native header forwarding policy differs','loopback_roundtrip_requests':4,'loopback_denial_requests':1,'real_http_requests':0})

def main():
 parser=argparse.ArgumentParser();mode=parser.add_mutually_exclusive_group(required=True);mode.add_argument('--offline',action='store_true');mode.add_argument('--execute',action='store_true');parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--offline-proof',type=Path);args=parser.parse_args()
 binary=args.binary.resolve();out=args.output.resolve();out.mkdir(mode=0o700,parents=True,exist_ok=False);os.chmod(ROOT/'wrapper.py',0o755)
 if digest(binary)!=RUNTIME_SHA:raise ValueError('runtime_identity')
 identity=source_identity()
 if args.offline:
  offline_gate(binary,out,identity)
  return
 proof=json.loads(args.offline_proof.read_text()) if args.offline_proof else {}
 if proof.get('passed') is not True or proof.get('source_files')!=identity or proof.get('runtime_sha256')!=RUNTIME_SHA:raise ValueError('offline_proof_binding')
 if not os.environ.get('GROK_API_KEY'):raise ValueError('credential_unavailable')
 results=[]
 with Relay(mode='real') as relay:
  try:
   def clean(r):return not r.get('cleanup_pending',False) and (r.get('passed') is True or r.get('failure',{}).get('http_status')==403)
   for arm in ['B','A']:
    results.append(run_arm(relay,arm,len(results)+1,binary,out))
    if not clean(results[-1]):break
  finally:write(out/'relay.json',relay.summary())
 write(out/'summary.json',{'runtime_sha256':RUNTIME_SHA,'artifact_id':12769806175,'model':'grok-4.6','experiment':'header_control_reverse','request_limit':4,'previous_real_requests':9,'cumulative_real_requests':9+relay.summary()['sent'],'results':results,'source_files':identity,'causal_conclusion':'exploratory only; no production oracle changed'})
if __name__=='__main__':
 try:main()
 except Exception as exc:
  safe_codes={'offline_gate_failed','runtime_identity','offline_roundtrip','offline_effort','offline_non_effort_delta','offline_zero_retry','offline_tool_token','offline_continuation','offline_proof_binding','credential_unavailable'}
  code=str(exc) if type(exc) is ValueError and str(exc) in safe_codes else 'experiment_failure'
  print('bounded experiment stopped: '+code);raise SystemExit(1)
