import hashlib
import http.client
import json
import unittest
from unittest import mock
from urllib.parse import urlsplit
import frozen
from relay import Relay

SECRET = 'private-auth-canary'
RAW = b'{ "model":"grok-4.6", "store":false, "stream":true,"reasoning":{"effort":"high"},"input":[] }'
HEADERS = {'Authorization': 'Bearer '+SECRET, 'Content-Type': 'application/json'}

class Response:
    def __init__(self, status): self.status = status
    def getheaders(self): return [('x-request-id','req-123'),('x-secret',SECRET)]
    def read(self, limit): return b'event: response.output_item.done\ndata: {"type":"response.completed","private":"'+SECRET.encode()+b'"}\n\n'

class Tests(unittest.TestCase):
    def test_capture_is_offline_and_default_cannot_send(self):
        subject=frozen.FrozenReplay()
        with mock.patch('http.client.HTTPSConnection',side_effect=AssertionError('no network')):
            with Relay(offline_transport=lambda body,headers:(403,{},b'{}'),raw_capture=subject.capture) as relay:
                relay.begin_arm('1-A','high')
                url=urlsplit(relay.url)
                conn=http.client.HTTPConnection(url.hostname,url.port)
                conn.request('POST','/v1/responses',RAW,HEADERS)
                response=conn.getresponse();self.assertEqual(response.status,403);response.read();conn.close()
            with self.assertRaises(ValueError): subject.run()
        self.assertEqual(subject.summary()['body_sha256'],hashlib.sha256(RAW).hexdigest())
        self.assertNotIn(SECRET,json.dumps(subject.summary()))
        self.assertEqual(subject.summary()['sent'],0)
        with self.assertRaises(ValueError): Relay(mode='real',raw_capture=subject.capture)

    def test_exact_bytes_headers_two_reservations_and_sixty_seconds(self):
        subject=frozen.FrozenReplay();subject.capture(RAW,HEADERS)
        calls=[];clock=[100.0]
        class Connection:
            def __init__(self,host,timeout):
                self.status=(403,200)[len(calls)]
                self.host=host
            def request(self,method,path,body,headers):
                calls.append((self.host,method,path,body,headers,clock[0],subject.summary()['sent']))
            def getresponse(self):return Response(self.status)
            def close(self):pass
        with mock.patch('http.client.HTTPSConnection',Connection), mock.patch('time.monotonic',side_effect=lambda:clock[0]), mock.patch('time.sleep',side_effect=lambda seconds:clock.__setitem__(0,clock[0]+seconds)):
            result=subject.run(enabled=True)
            with self.assertRaises(ValueError):subject.run(enabled=True)
        self.assertEqual(result['sent'],2)
        self.assertTrue(all(r['body_sha256']==hashlib.sha256(RAW).hexdigest() and r['header_values_equal_to_frozen'] for r in result['requests']))
        self.assertGreaterEqual(result['requests'][1]['seconds_after_previous_completion'],60)
        self.assertEqual([r['status'] for r in result['requests']],[403,200])
        self.assertEqual(calls[0][:5],calls[1][:5])
        self.assertEqual(calls[0][:5],(frozen.HOST,'POST',frozen.PATH,RAW,HEADERS))
        self.assertEqual([c[-1] for c in calls],[1,2])
        self.assertGreaterEqual(calls[1][5]-calls[0][5],60)
        self.assertNotIn(SECRET,json.dumps(result))
        self.assertIn('response.completed',result['requests'][0]['sse_types'])

    def test_uncertain_or_redirect_does_not_send_again(self):
        for status in (302,500,None):
            subject=frozen.FrozenReplay();subject.capture(RAW,HEADERS)
            connection=mock.Mock()
            if status is None:connection.request.side_effect=TimeoutError(SECRET)
            else:connection.getresponse.return_value=Response(status)
            with mock.patch('http.client.HTTPSConnection',return_value=connection) as factory, mock.patch('time.sleep') as sleep:
                result=subject.run(enabled=True)
            self.assertEqual(factory.call_count,1);sleep.assert_not_called()
            self.assertEqual(result['sent'],1)
            self.assertNotIn(SECRET,json.dumps(result))
            self.assertEqual(result['requests'][0]['state'],'transport_unknown' if status is None else 'stopped_status')

    def test_capture_rejects_noninitial_or_stored_request(self):
        for change in ({'store':True},{'reasoning':{}},{'input':[{'type':'function_call'}]}, {'input':[{'type':'reasoning','encrypted_content':SECRET}]}):
            body=json.loads(RAW);body.update(change)
            with self.assertRaises(ValueError):frozen.FrozenReplay().capture(json.dumps(body).encode(),HEADERS)
        subject=frozen.FrozenReplay();subject.capture(RAW,HEADERS)
        with self.assertRaises(ValueError):subject.capture(RAW,HEADERS)

if __name__=='__main__':unittest.main()
