import http.client
import json
import threading
import unittest
from unittest import mock
from urllib.parse import urlsplit
from relay import Relay

CANARY='CREDENTIAL_PRIVATE_CANARY'
def send(relay, effort=None, path='/v1/responses', continuation=False):
    url=urlsplit(relay.url)
    conn=http.client.HTTPConnection(url.hostname,url.port,timeout=5)
    body={'model':'grok-4.6','reasoning':{} if effort is None else {'effort':effort},'input':[{'type':'function_call','call_id':'call-1'},{'type':'function_call_output','call_id':'call-1','output':CANARY}] if continuation else []}
    conn.request('POST',path,json.dumps(body),{'Authorization':'Bearer '+CANARY,'Content-Type':'application/json'})
    response=conn.getresponse(); data=response.read(); conn.close()
    return response.status,data

class Tests(unittest.TestCase):
    def test_budget_effort_and_safe_summary(self):
        calls=[]
        def transport(body,headers):
            calls.append(body['reasoning'])
            self.assertEqual(headers['Authorization'],'Bearer '+CANARY)
            return 200,{'x-request-id':'safe-123','x-secret':CANARY},b'data: {"type":"response.completed"}\n\n'
        with mock.patch('http.client.HTTPSConnection',side_effect=AssertionError('offline network')), Relay(offline_transport=transport) as relay:
            for i,effort in enumerate((None,'high','high',None),1):
                relay.begin_arm(str(i)+('-A' if effort is None else '-B'),effort)
                self.assertEqual(send(relay,effort)[0],200)
                self.assertEqual(send(relay,effort,continuation=True)[0],200)
                self.assertEqual(send(relay,effort)[0],409)
                relay.end_arm()
            self.assertEqual(relay.summary()['sent'],8)
            self.assertEqual(len(calls),8)
            self.assertNotIn(CANARY,json.dumps(relay.summary()))
            with self.assertRaises(ValueError): relay.begin_arm('1-A',None)

    def test_failure_redirect_and_timeout_never_resend(self):
        for result in ((403,{},CANARY.encode()),(302,{'Location':'https://elsewhere.invalid'},b''),(200,{},b'data: {"type":"error"}\n\n'),TimeoutError()):
            with self.subTest(result=type(result).__name__):
                calls=[]
                def transport(body,headers):
                    calls.append(1)
                    if isinstance(result,Exception): raise result
                    return result
                with Relay(offline_transport=transport) as relay:
                    relay.begin_arm('1-A',None)
                    status,data=send(relay)
                    self.assertNotIn(CANARY.encode(),data)
                    self.assertEqual(send(relay)[0],409)
                    self.assertEqual(len(calls),1)
                    self.assertTrue(relay.summary()['arms']['1-A']['failed'])
                    self.assertEqual(relay.summary()['sent'],1)

    def test_invalid_route_effort_and_offline_default(self):
        with Relay() as relay:
            relay.begin_arm('1-A',None)
            self.assertEqual(send(relay,path='/v1/models')[0],400)
            self.assertEqual(send(relay,'high')[0],400)
            self.assertEqual(relay.summary()['sent'],0)
        with Relay() as relay:
            relay.begin_arm('1-A',None)
            with mock.patch('http.client.HTTPSConnection',side_effect=AssertionError('offline network')):
                self.assertEqual(send(relay)[0],502)

    def test_second_initial_request_is_blocked_not_retried(self):
        calls=[]
        def transport(body,headers):
            calls.append(1)
            return 200,{},b'data: {"type":"response.completed"}\n\n'
        with Relay(offline_transport=transport) as relay:
            relay.begin_arm('1-A',None)
            self.assertEqual(send(relay)[0],200)
            self.assertEqual(send(relay)[0],400)
            self.assertEqual(len(calls),1)
            self.assertTrue(relay.summary()['arms']['1-A']['failed'])

    def test_concurrent_reservation_cannot_bypass_arm(self):
        entered=threading.Event();release=threading.Event();calls=[]
        def transport(body,headers):
            calls.append(1);entered.set();release.wait(3)
            return 403,{},b''
        with Relay(offline_transport=transport) as relay:
            relay.begin_arm('1-A',None)
            thread=threading.Thread(target=lambda:send(relay));thread.start()
            self.assertTrue(entered.wait(2))
            self.assertEqual(send(relay)[0],409)
            with self.assertRaises(ValueError): relay.end_arm()
            release.set();thread.join()
            self.assertEqual(send(relay)[0],409)
            self.assertEqual(len(calls),1)

if __name__=='__main__':unittest.main()
