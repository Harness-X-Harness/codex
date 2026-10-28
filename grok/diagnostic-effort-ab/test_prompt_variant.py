import copy
import os
import json
from pathlib import Path
import unittest
import prompt_variant as p

SECRET = 'private-fixture-canary'

def body():
    return {'model':'grok-4.6','stream':True,'store':False,
            'reasoning':{'effort':'high'},'prompt_cache_key':SECRET,
            'tools':[{'name':'grok_history_probe','description':'Return a fresh token for this history probe.'}],
            'text':{'format':{'schema':{'properties':{'token':{'type':['string','null']}}}}},
            'input':[{'type':'message','role':'developer','content':[{'type':'input_text','text':SECRET}]},
                     {'type':'message','role':'user','content':[{'type':'input_text','text':p.PROMPT_A}]}]}

def raw(value): return json.dumps(value, indent=2).encode()

class Tests(unittest.TestCase):
    def test_only_prompt_bytes_change_and_safe_summary(self):
        a = raw(body()); b, proof = p.prepare(a)
        expected = body(); expected['input'][1]['content'][0]['text'] = p.PROMPT_B
        self.assertEqual(json.loads(b), expected)
        self.assertEqual(b.replace(json.dumps(p.PROMPT_B).encode(),json.dumps(p.PROMPT_A).encode()),a)
        self.assertNotIn(SECRET,json.dumps(proof))
        self.assertEqual(proof['changed_fields'],1)
        self.assertTrue(p.PROMPT_B.endswith(p.PROMPT_A))
    def test_prefix_cannot_overflow_request_budget(self):
        value = body(); value['padding'] = ''
        value['padding'] = 'x' * (p.MAX_BYTES - len(raw(value)))
        encoded = raw(value)
        self.assertEqual(len(encoded), p.MAX_BYTES)
        with self.assertRaisesRegex(ValueError, 'modified_body_bounds'):
            p.prepare(encoded)

    def test_does_not_force_effort(self):
        value = body(); del value['reasoning']
        b, _ = p.prepare(raw(value)); self.assertNotIn('reasoning',json.loads(b))
    def test_reject_duplicate_or_missing_or_nonstandalone_task(self):
        variants=[]
        v=body(); v['input'].append(copy.deepcopy(v['input'][-1])); variants.append(v)
        v=body(); v['input'].pop(); variants.append(v)
        v=body(); v['input'][-1]['content'].append({'type':'input_text','text':'extra'}); variants.append(v)
        v=body(); v['extra']=p.PROMPT_A; variants.append(v)
        for v in variants:
            with self.subTest(v=v), self.assertRaises(ValueError): p.prepare(raw(v))
    def test_reject_continuations_duplicate_keys_and_nonfinite(self):
        for kind in ['function_call','function_call_output','reasoning']:
            v=body(); v['input'].append({'type':kind})
            with self.assertRaises(ValueError): p.prepare(raw(v))
        for bad in [b'{"input":[],"input":[]}',b'{"x":NaN}',b'',b'[]']:
            with self.assertRaises(ValueError): p.prepare(bad)
    def test_binding_to_exact_formal_source(self):
        root=Path(os.environ.get('FORMAL_HARNESS_ROOT','/workspace/shared/grok164-prompt-diagnosis'))/'grok'
        self.assertIn('Text: '+json.dumps(p.PROMPT_A), (root/'live/sdk_reasoning.go').read_text())
        observer=(root/'live/sdk_reasoning_observer.go').read_text()
        self.assertIn('rand.Read(nonce[:])',observer)
        self.assertIn('hex.EncodeToString(nonce[:])',observer)

if __name__=='__main__': unittest.main()
