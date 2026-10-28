import json,os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import run
class FakeRelay:
 def __init__(self,**kwargs):pass
 def __enter__(self):return self
 def __exit__(self,*args):pass
 def summary(self):return {'real_http_requests':0}
class ControllerTests(unittest.TestCase):
 def check(self,results,want):
  with tempfile.TemporaryDirectory() as t:
   root=Path(t); proof=root/'proof.json';proof.write_text(json.dumps({'passed':True,'source_files':{},'runtime_sha256':run.RUNTIME_SHA})); calls=[]
   def arm(relay,name,*args):calls.append(name);return results[len(calls)-1]
   with patch.object(sys,'argv',['run.py','--execute','--binary',str(root/'unused'),'--output',str(root/'out'),'--offline-proof',str(proof)]),patch.object(run,'digest',return_value=run.RUNTIME_SHA),patch.object(run,'source_identity',return_value={}),patch.object(run,'Relay',FakeRelay),patch.object(run,'run_arm',side_effect=arm),patch.dict(os.environ,{'GROK_API_KEY':'CREDENTIAL_CANARY'}):run.main()
   self.assertEqual(calls,want)
   self.assertNotIn('CREDENTIAL_CANARY',(root/'out'/'summary.json').read_text())
 def test_both_denied_stop_after_pair(self):self.check([{'passed':False,'failure':{'http_status':403}}]*2,['A','B'])
 def test_difference_reverses_once(self):self.check([{'passed':False,'failure':{'http_status':403}},{'passed':True},{'passed':True},{'passed':False,'failure':{'http_status':403}}],['A','B','B','A'])
 def test_unknown_stops_immediately(self):self.check([{'passed':False,'failure':{'code':'driver_timeout'}}],['A'])
 def test_only_explicit_ephemera_normalized(self):
  a={'prompt_cache_key':'a','reasoning':{},'input':[{'type':'function_call_output','output':'a'*64},{'text':'/tmp/grok-live-123/workspace'}]}
  b={'prompt_cache_key':'b','reasoning':{'effort':'high'},'input':[{'type':'function_call_output','output':'b'*64},{'text':'/tmp/grok-live-456/workspace'}]}
  self.assertEqual(run.normalized(a),run.normalized(b));b['extra']='different';self.assertNotEqual(run.normalized(a),run.normalized(b))
if __name__=='__main__':unittest.main()
