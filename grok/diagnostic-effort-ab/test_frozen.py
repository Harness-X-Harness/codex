import hashlib
import http.client
import json
import unittest
from unittest import mock
from urllib.parse import urlsplit
import frozen
from relay import Relay

SECRET = 'private-auth-canary'
TASK = {'id':'original-message', 'type':'message', 'role':'user', 'content':[{'type':'input_text','text':frozen.TASK_PROMPT}]}
RAW = json.dumps({'model':'grok-4.6','store':False,'stream':True,'reasoning':{'effort':'high'},'tools':[{'type':'function','name':'probe'}],'input':[{'type':'message','role':'developer','content':[{'type':'input_text','text':SECRET}]},TASK]}, indent=1).encode()
HEADERS = {'Authorization': 'Bearer '+SECRET, 'Content-Type': 'application/json', 'thread-id':'fixed-thread'}

class Response:
    def __init__(self,status=200,failed=False,text='Okay.'):
        self.status,self.failed,self.text=status,failed,text
    def getheaders(self): return [('x-request-id','req-123'),('x-secret',SECRET)]
    def read(self,limit):
        frame={'type':'response.failed' if self.failed else 'response.completed','response':{
            'status':'failed' if self.failed else 'completed','error':{'message':SECRET},
            'output':[{'type':'reasoning','encrypted_content':SECRET,'text':SECRET},
                      {'type':'message','role':'assistant','content':[{'type':'reasoning_text','text':SECRET},
                       {'type':'output_text','text':self.text}]}]}}
        return b'data: '+json.dumps(frame).encode()+b'\n\n'

class Tests(unittest.TestCase):
    def setUp(self):
        self.guard=mock.patch.object(frozen,'_batch_started',False)
        self.guard.start();self.addCleanup(self.guard.stop)
    def full(self):
        f=frozen.FrozenReplay();f.capture(RAW,HEADERS);return f

    def test_capture_is_offline_and_default_cannot_send(self):
        subject=frozen.FrozenReplay()
        with mock.patch('http.client.HTTPSConnection',side_effect=AssertionError('no network')):
            with Relay(offline_transport=lambda body,headers:(403,{},b'{}'),raw_capture=subject.capture) as relay:
                relay.begin_arm('1-A','high');url=urlsplit(relay.url)
                conn=http.client.HTTPConnection(url.hostname,url.port)
                conn.request('POST','/v1/responses',RAW,HEADERS)
                response=conn.getresponse();self.assertEqual(response.status,403);response.read();conn.close()
            with self.assertRaises(ValueError):frozen.run_fcf(subject,{})
        self.assertEqual(subject.summary()['body_sha256'],hashlib.sha256(RAW).hexdigest())
        self.assertNotIn(SECRET,json.dumps(subject.summary()))
        with self.assertRaises(ValueError):Relay(mode='real',raw_capture=subject.capture)

    def test_exact_fcf_three_requests_private_filter_and_intervals(self):
        full=self.full();calls=[];clock=[100.0];report={}
        class Connection:
            def __init__(self,host,timeout):self.host=host
            def request(self,method,path,body,headers):
                calls.append((self.host,method,path,body,headers,report['sent']))
            def getresponse(self):return Response(text=SECRET if len(calls)==2 else 'Okay.')
            def close(self):pass
        with mock.patch('http.client.HTTPSConnection',Connection),mock.patch('time.monotonic',side_effect=lambda:clock[0]),mock.patch('time.sleep',side_effect=lambda s:clock.__setitem__(0,clock[0]+s)):
            frozen.run_fcf(full,report,enabled=True)
            with self.assertRaises(ValueError):frozen.run_fcf(self.full(),{},enabled=True)
        self.assertEqual(report['sent'],3)
        self.assertEqual([x['arm'] for x in report['requests']],list('FCF'))
        self.assertEqual([x[-1] for x in calls],[1,2,3])
        self.assertEqual(calls[0][:5],calls[2][:5])
        self.assertEqual(calls[0][3],RAW)
        context=json.loads(calls[1][3]);original=json.loads(RAW)
        self.assertEqual(context['input'],[TASK])
        self.assertEqual({k:v for k,v in context.items() if k!='input'}, {k:v for k,v in original.items() if k!='input'})
        self.assertTrue(all(call[4]==HEADERS for call in calls))
        self.assertTrue(all(x[:3]==(frozen.HOST,'POST',frozen.PATH) for x in calls))
        self.assertTrue(all(r['body_sha256']==report['captures'][r['arm']]['body_sha256'] and r['header_values_equal_to_frozen'] for r in report['requests']))
        self.assertTrue(all(r['seconds_after_previous_completion']>=60 for r in report['requests'][1:]))
        self.assertTrue(all('public_output_text' not in r for r in report['requests']))
        self.assertNotIn(SECRET,json.dumps(report))
        self.assertTrue(all(r['completion']=='completed' for r in report['requests']))

    def test_http200_failed_is_not_completed_and_no_response_text_retained(self):
        clock=[0.];connection=mock.Mock()
        connection.getresponse.side_effect=[Response(failed=True),Response(text=SECRET),Response(text='x'*300)]
        with mock.patch('http.client.HTTPSConnection',return_value=connection),mock.patch('time.monotonic',side_effect=lambda:clock[0]),mock.patch('time.sleep',side_effect=lambda s:clock.__setitem__(0,clock[0]+s)):
            report=frozen.run_fcf(self.full(),{},enabled=True)
        self.assertEqual(report['requests'][0]['status'],200)
        self.assertEqual(report['requests'][0]['completion'],'failed')
        self.assertTrue(all('public_output_text' not in r for r in report['requests']))
        self.assertNotIn(SECRET,json.dumps(report))

    def test_native_done_message_and_completed_id_only_are_complete(self):
        message={'id':'message-1','type':'message','role':'assistant',
                 'content':[{'type':'output_text','text':'Fixture public reply'}]}
        class NativeResponse(Response):
            def __init__(self,terminal_output=False):super().__init__();self.terminal_output=terminal_output
            def read(self,limit):
                response={'id':'response-1'}
                if self.terminal_output:response['output']=[message]
                return b''.join(b'data: '+json.dumps(frame).encode()+b'\n\n' for frame in (
                    {'type':'response.output_item.done','output_index':0,'item':message},
                    {'type':'response.completed','response':response}))
        connection=mock.Mock();connection.getresponse.side_effect=[NativeResponse(),NativeResponse(),NativeResponse(True)]
        clock=[0.]
        with mock.patch('http.client.HTTPSConnection',return_value=connection),mock.patch('time.monotonic',side_effect=lambda:clock[0]),mock.patch('time.sleep',side_effect=lambda s:clock.__setitem__(0,clock[0]+s)):
            report=frozen.run_fcf(self.full(),{},enabled=True)
        self.assertEqual(report['sent'],3)
        self.assertTrue(all(item['completion']=='completed' for item in report['requests']))
        self.assertNotIn('Fixture public reply',json.dumps(report))
        self.assertTrue(all('public_output_text' not in r for r in report['requests']))

    def test_uncertain_or_redirect_does_not_send_again(self):
        for status in (302,500,None):
            self.guard.stop();self.guard.start();connection=mock.Mock()
            if status is None:connection.request.side_effect=TimeoutError(SECRET)
            else:connection.getresponse.return_value=Response(status)
            with mock.patch('http.client.HTTPSConnection',return_value=connection) as factory,mock.patch('time.sleep') as sleep:
                report=frozen.run_fcf(self.full(),{},enabled=True)
            self.assertEqual(factory.call_count,1);sleep.assert_not_called()
            self.assertEqual(report['sent'],1);self.assertNotIn(SECRET,json.dumps(report))

    def test_interrupted_wait_preserves_safe_partial_ledger(self):
        report={};connection=mock.Mock();connection.getresponse.return_value=Response(403)
        with mock.patch('http.client.HTTPSConnection',return_value=connection) as network,mock.patch('time.sleep',side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):frozen.run_fcf(self.full(),report,enabled=True)
        self.assertEqual(network.call_count,1);self.assertEqual(report['sent'],1)
        self.assertEqual(report['requests'][0]['status'],403)
        self.assertNotIn(SECRET,json.dumps(report))
        with self.assertRaises(ValueError):frozen.run_fcf(self.full(),{},enabled=True)

    def test_context_gate_rejects_missing_duplicate_and_mixed_messages_before_network(self):
        mixed=dict(TASK,content=TASK['content']+[{'type':'input_text','text':'environment'}])
        substring=dict(TASK,content=[{'type':'input_text','text':'environment '+frozen.TASK_PROMPT}])
        for inputs in ([],[TASK,TASK],[mixed],[substring],[TASK,mixed],[TASK]):
            body=json.loads(RAW);body['input']=inputs
            subject=frozen.FrozenReplay();subject.capture(json.dumps(body).encode(),HEADERS)
            with mock.patch('http.client.HTTPSConnection') as network:
                with self.assertRaises(ValueError):frozen.prepare_context(subject)
                with self.assertRaises(ValueError):frozen.run_fcf(subject,{},enabled=True)
                network.assert_not_called()
        with mock.patch('http.client.HTTPSConnection') as network:
            proof=frozen.prepare_context(self.full());network.assert_not_called()
        self.assertTrue(proof['non_input_fields_equal']);self.assertTrue(proof['task_message_preserved'])
        self.assertEqual(proof['context_input_count'],1)
        self.assertNotIn(SECRET,json.dumps(proof));self.assertNotIn(frozen.TASK_PROMPT,json.dumps(proof))

    def test_capture_rejects_noninitial_or_stored_request(self):
        for change in ({'store':True},{'reasoning':{}},{'input':[{'type':'function_call'}]},{'input':[{'type':'reasoning','encrypted_content':SECRET}]}):
            body=json.loads(RAW);body.update(change)
            with self.assertRaises(ValueError):frozen.FrozenReplay().capture(json.dumps(body).encode(),HEADERS)
        subject=self.full()
        with self.assertRaises(ValueError):subject.capture(RAW,HEADERS)
        copied=subject.headers();copied['Authorization']='changed'
        self.assertEqual(subject.headers(),HEADERS)

if __name__=='__main__':unittest.main()
