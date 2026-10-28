import json
import unittest
from unittest import mock
import frozen
from test_prompt_variant import body, raw, SECRET

HEADERS={'Authorization':'Bearer '+SECRET,'Content-Type':'application/json','thread-id':'fixed'}
class Response:
    status=200
    def getheaders(self): return [('x-request-id','req-123'),('x-secret',SECRET)]
    def read(self,limit): return b'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
class Tests(unittest.TestCase):
    def setUp(self):
        patcher=mock.patch.object(frozen,'_batch_started',False);patcher.start();self.addCleanup(patcher.stop)
    def capture(self):
        f=frozen.FrozenReplay(); f.capture(raw(body()),HEADERS);return f
    def test_disabled_by_default(self):
        with mock.patch('http.client.HTTPSConnection',side_effect=AssertionError('network')):
            with self.assertRaises(ValueError): frozen.run_abba(self.capture(),{})
    def test_abba_budget_same_headers_and_one_field_only(self):
        f=self.capture(); calls=[];clock=[100.0];report={}
        class Connection:
            def __init__(self,host,timeout): pass
            def request(self,method,path,body,headers): calls.append((body,headers,report['sent']))
            def getresponse(self): return Response()
            def close(self): pass
        with mock.patch('http.client.HTTPSConnection',Connection), mock.patch('time.monotonic',side_effect=lambda:clock[0]), mock.patch('time.sleep',side_effect=lambda s:clock.__setitem__(0,clock[0]+s)):
            frozen.run_abba(f,report,enabled=True)
            with self.assertRaises(ValueError): frozen.run_abba(self.capture(),{},enabled=True)
        self.assertEqual(report['sent'],4); self.assertEqual([x['arm'] for x in report['requests']],list('ABBA'))
        self.assertEqual(calls[0][0],calls[3][0]);self.assertEqual(calls[1][0],calls[2][0]);self.assertNotEqual(calls[0][0],calls[1][0])
        self.assertEqual([x[2] for x in calls],[1,2,3,4]);self.assertTrue(all(x[1]==HEADERS for x in calls))
        self.assertTrue(all(x['seconds_after_previous_completion']>=60 for x in report['requests'][1:]))
        self.assertNotIn(SECRET,json.dumps(report))
    def test_transport_unknown_consumes_reservation_and_stops(self):
        report={}
        with mock.patch('http.client.HTTPSConnection',side_effect=OSError(SECRET)):
            frozen.run_abba(self.capture(),report,enabled=True)
        self.assertEqual(report['sent'],1); self.assertEqual(report['requests'][0]['state'],'transport_unknown')
        self.assertNotIn(SECRET,json.dumps(report))
    def test_unexpected_status_stops(self):
        response=Response();response.status=429
        conn=mock.Mock();conn.getresponse.return_value=response;report={}
        with mock.patch('http.client.HTTPSConnection',return_value=conn): frozen.run_abba(self.capture(),report,enabled=True)
        self.assertEqual(report['sent'],1); self.assertEqual(report['requests'][0]['state'],'stopped_status')
    def test_403_classification_is_allowlisted(self):
        response=Response();response.status=403;response.read=lambda n:json.dumps({'code':'permission-denied','message':SECRET}).encode()
        conn=mock.Mock();conn.getresponse.return_value=response;report={}
        with mock.patch('http.client.HTTPSConnection',return_value=conn),mock.patch('time.sleep'),mock.patch('time.monotonic',side_effect=range(0,100000,100)):
            frozen.run_abba(self.capture(),report,enabled=True)
        self.assertEqual(report['sent'],4);self.assertTrue(all(x['error_class']=='permission-denied' for x in report['requests']))
        self.assertNotIn(SECRET,json.dumps(report))
class RegressionTests(unittest.TestCase):
    setUp = Tests.setUp
    capture = Tests.capture
    def test_200_failed_incomplete_and_native_sse_are_distinguished(self):
        frames = [
            {'type':'response.failed','response':{'status':'failed','output':SECRET}},
            {'type':'response.incomplete','response':{'status':'incomplete','output':SECRET}},
            {'type':'response.completed','response':{'id':'id-only'}},
            {'type':'response.completed','response':{'status':'completed','output':SECRET}},
        ]
        responses=[]
        for frame in frames:
            response=Response(); response.read=lambda n, frame=frame: b'data: '+json.dumps(frame).encode()+b'\n\n'
            responses.append(response)
        conn=mock.Mock();conn.getresponse.side_effect=responses;report={}
        with mock.patch('http.client.HTTPSConnection',return_value=conn),mock.patch('time.sleep'),mock.patch('time.monotonic',side_effect=range(0,100000,100)):
            frozen.run_abba(self.capture(),report,enabled=True)
        self.assertEqual([x['completion'] for x in report['requests']],['failed','incomplete','completed','completed'])
        self.assertNotIn(SECRET,json.dumps(report))
    def test_interrupted_wait_preserves_ledger_without_resend(self):
        response=Response();response.status=403
        conn=mock.Mock();conn.getresponse.return_value=response;report={}
        with mock.patch('http.client.HTTPSConnection',return_value=conn) as network,mock.patch('time.sleep',side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):frozen.run_abba(self.capture(),report,enabled=True)
        self.assertEqual(network.call_count,1);self.assertEqual(report['sent'],1)
        with self.assertRaises(ValueError):frozen.run_abba(self.capture(),{},enabled=True)
    def test_noninitial_stored_and_second_capture_rejected(self):
        for change in ({'store':True},{'input':[{'type':'function_call'}]},{'input':[{'type':'reasoning','encrypted_content':SECRET}]}):
            value=body();value.update(change)
            with self.assertRaises(ValueError):frozen.FrozenReplay().capture(raw(value),HEADERS)
        f=self.capture()
        with self.assertRaises(ValueError):f.capture(raw(body()),HEADERS)
        headers=f.headers();headers['Authorization']='changed';self.assertEqual(f.headers(),HEADERS)
    def test_response_oversize_or_close_failure_stops(self):
        for oversized in [True,False]:
            with self.subTest(oversized=oversized),mock.patch.object(frozen,'_batch_started',False):
                response=Response()
                if oversized:response.read=lambda n:b'x'*(frozen.MAX_BYTES+1)
                conn=mock.Mock();conn.getresponse.return_value=response
                if not oversized:conn.close.side_effect=OSError(SECRET)
                report={}
                with mock.patch('http.client.HTTPSConnection',return_value=conn):frozen.run_abba(self.capture(),report,enabled=True)
                self.assertEqual(report['sent'],1);self.assertEqual(report['requests'][0]['state'],'transport_unknown')
                self.assertNotIn(SECRET,json.dumps(report))
    def test_modified_request_oversize_blocks_before_network(self):
        value=body();value['padding']='';value['padding']='x'*(frozen.MAX_BYTES-len(raw(value)))
        f=frozen.FrozenReplay();f.capture(raw(value),HEADERS)
        with mock.patch('http.client.HTTPSConnection') as network, mock.patch('prompt_variant.PROMPT_B','x'*1024):
            with self.assertRaisesRegex(ValueError,'modified_body_bounds'):frozen.run_abba(f,{},enabled=True)
        network.assert_not_called()

if __name__=='__main__':unittest.main()
