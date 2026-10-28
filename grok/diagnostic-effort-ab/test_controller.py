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
class OfflineDiagnosticsTests(unittest.TestCase):
 def test_difference_report_does_not_dump_values(self):
  report=run.difference_report({'input':[{'text':'SECRET_CANARY grok-live-123'}]}, {'input':[{'text':'PRIVATE_CIPHER_CANARY'}]})
  encoded=json.dumps(report)
  self.assertEqual(report['count'],1);self.assertEqual(report['differences'][0]['path'],'/input/0/text')
  self.assertNotIn('SECRET_CANARY',encoded);self.assertNotIn('PRIVATE_CIPHER_CANARY',encoded)
 def test_failed_comparison_still_records_denial_and_all_checks(self):
  calls=[]
  class LocalRelay(FakeRelay):
   def __init__(self,**kwargs):self.transport=kwargs['offline_transport']
  def arm(relay,name,ordinal,*args):
   calls.append(ordinal)
   if ordinal==3:
    relay.transport({},{});return {'passed':False,'cleanup_pending':False,'failure':{'http_status':403}}
   body={'model':'grok-4.6','reasoning':{} if name=='A' else {'effort':'high'},'input':[{'type':'message','text':'DIFFERENT_PRIVATE_'+name}]}
   # Capture shape without relying on SSE, which is not part of this unit test.
   relay.transport(body,{})
   second={**body,'input':[{'type':'function_call','call_id':'x'},{'type':'function_call_output','call_id':'x','output':'a'*64},{'type':'reasoning','encrypted_content':'opaque-offline-cipher'}]}
   relay.transport(second,{})
   return {'passed':True,'cleanup_pending':False}
  with tempfile.TemporaryDirectory() as tmp,patch.object(run,'Relay',LocalRelay),patch.object(run,'run_arm',side_effect=arm),patch.object(run,'sse',return_value=b''):
   out=Path(tmp)
   with self.assertRaisesRegex(ValueError,'offline_gate_failed'):run.offline_gate(Path('/unused'),out,{})
   report=json.loads((out/'offline-report.json').read_text())
   self.assertEqual(calls,[1,2,3]);self.assertTrue(report['checks']['zero_retry'])
   self.assertFalse(report['checks']['only_effort_delta']);self.assertEqual(len(report['comparisons']),2)
   self.assertNotIn('DIFFERENT_PRIVATE',json.dumps(report));self.assertFalse((out/'proof.json').exists())
if __name__=='__main__':unittest.main()
