import json,os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import run
class FakeRelay:
 def __init__(self,**kwargs):pass
 def __enter__(self):return self
 def __exit__(self,*args):pass
 def summary(self):return {'sent':0}
class ControllerTests(unittest.TestCase):
 def test_main_only_runs_one_abba_batch_and_reports_budget(self):
  with tempfile.TemporaryDirectory() as t:
   root=Path(t);proof=root/'proof.json';proof.write_text(json.dumps({'passed':True,'full_initial_capture_passed':True,'prompt_only_gate_passed':True,'source_files':{},'runtime_sha256':run.RUNTIME_SHA}))
   with patch.object(sys,'argv',['run.py','--execute','--binary',str(root/'unused'),'--output',str(root/'out'),'--offline-proof',str(proof)]),patch.object(run,'digest',return_value=run.RUNTIME_SHA),patch.object(run,'source_identity',return_value={}),patch.object(run,'capture_initial',return_value='full') as capture,patch.object(run,'prepare_prompt',return_value=(b'unused',{'changed_field':'prompt'})),patch.object(run,'run_abba',side_effect=lambda full,report,**kw:report.update(sent=4)) as pair,patch.dict(os.environ,{'GROK_API_KEY':'CREDENTIAL_CANARY'}):run.main()
   self.assertEqual(capture.call_count,1);pair.assert_called_once_with('full',{'sent':4},enabled=True)
   summary=json.loads((root/'out'/'summary.json').read_text())
   self.assertEqual(summary['request_limit'],4);self.assertEqual(summary['cumulative_real_requests'],31)
   self.assertNotIn('CREDENTIAL_CANARY',json.dumps(summary))
 def test_missing_capture_gate_stops_before_any_capture(self):
  with tempfile.TemporaryDirectory() as t:
   root=Path(t);proof=root/'proof.json';proof.write_text(json.dumps({'passed':True,'source_files':{},'runtime_sha256':run.RUNTIME_SHA}))
   with patch.object(sys,'argv',['run.py','--execute','--binary',str(root/'unused'),'--output',str(root/'out'),'--offline-proof',str(proof)]),patch.object(run,'digest',return_value=run.RUNTIME_SHA),patch.object(run,'source_identity',return_value={}),patch.object(run,'capture_initial') as capture:
    with self.assertRaisesRegex(ValueError,'offline_proof_binding'):run.main()
   capture.assert_not_called()
 def test_only_explicit_ephemera_normalized(self):
  a={'prompt_cache_key':'a','reasoning':{},'input':[{'type':'function_call_output','output':'a'*64},{'text':'/tmp/grok-live-123/workspace'}]}
  b={'prompt_cache_key':'b','reasoning':{},'input':[{'type':'function_call_output','output':'b'*64},{'text':'/tmp/grok-live-456/workspace'}]}
  self.assertEqual(run.normalized(a),run.normalized(b));b['extra']='different';self.assertNotEqual(run.normalized(a),run.normalized(b))
 def test_prompt_and_effort_are_not_normalized(self):
  a={'reasoning':{},'input':[{'type':'message','role':'user','content':[{'type':'input_text','text':run.PROMPT_A}]}]}
  b={'reasoning':{},'input':[{'type':'message','role':'user','content':[{'type':'input_text','text':run.PROMPT_A}]}]}
  self.assertEqual(run.normalized(a),run.normalized(b))
  b['reasoning']['effort']='high';self.assertNotEqual(run.normalized(a),run.normalized(b))
  b['reasoning']={};b['input'][0]['role']='developer';self.assertNotEqual(run.normalized(a),run.normalized(b))
class OfflineDiagnosticsTests(unittest.TestCase):
 def test_difference_report_does_not_dump_values(self):
  report=run.difference_report({'input':[{'text':'SECRET_CANARY grok-live-123'}]}, {'input':[{'text':'PRIVATE_CIPHER_CANARY'}]})
  encoded=json.dumps(report)
  self.assertEqual(report['count'],1);self.assertEqual(report['differences'][0]['path'],'/input/0/text')
  self.assertNotIn('SECRET_CANARY',encoded);self.assertNotIn('PRIVATE_CIPHER_CANARY',encoded)
 def test_item_identity_normalization_preserves_replay_graph_and_call_id(self):
  a={'input':[{'type':'message','id':'msg_11111111-1111-1111-1111-111111111111'}]}
  b={'input':[{'type':'message','id':'msg_22222222-2222-2222-2222-222222222222'}]}
  left={};right={}
  self.assertEqual(run.normalized(a,left),run.normalized(b,right))
  self.assertEqual(run.normalized(a,left),run.normalized(b,right))
  b['input'][0]['id']='msg_33333333-3333-3333-3333-333333333333'
  self.assertNotEqual(run.normalized(a,left),run.normalized(b,right))
  self.assertNotEqual(run.normalized({'input':[{'type':'function_call','call_id':'a'}]}),run.normalized({'input':[{'type':'function_call','call_id':'b'}]}))
  self.assertNotEqual(run.normalized({'input':[{'type':'message','id':'unrecognized-a'}]}),run.normalized({'input':[{'type':'message','id':'unrecognized-b'}]}))
 def test_failed_comparison_still_records_denial_and_all_checks(self):
  calls=[]
  class LocalRelay(FakeRelay):
   def __init__(self,**kwargs):self.transport=kwargs['offline_transport']
  def arm(relay,name,ordinal,*args):
   calls.append(ordinal)
   if ordinal==3:
    relay.transport({},{});return {'passed':False,'cleanup_pending':False,'failure':{'http_status':403}}
   body={'model':'grok-4.6','reasoning':{},'input':[{'type':'message','text':'DIFFERENT_PRIVATE_'+name}]}
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
   self.assertFalse(report['checks']['profile_binding']);self.assertEqual(len(report['comparisons']),2)
   self.assertNotIn('DIFFERENT_PRIVATE',json.dumps(report));self.assertFalse((out/'proof.json').exists())
if __name__=='__main__':unittest.main()
