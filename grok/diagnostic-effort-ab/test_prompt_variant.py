import copy
import os
import json
from pathlib import Path
import unittest
from unittest import mock
import prompt_variant as p

SECRET = 'private-fixture-canary'

def body():
    return {'model':'grok-4.6','stream':True,'store':False,
            'reasoning':{'effort':'high'},'prompt_cache_key':SECRET,
            'tools':[{'type':'function','name':'grok_history_probe','description':p.DESCRIPTION_A,'parameters':copy.deepcopy(p.PARAMETERS)}],
            'text':{'format':{'schema':copy.deepcopy(p.SCHEMA_A)}},
            'input':[{'type':'message','role':'developer','content':[{'type':'input_text','text':SECRET}]},
                     {'type':'message','role':'user','content':[{'type':'input_text','text':p.PROMPT_A}]}]}

def raw(value): return json.dumps(value, indent=2).encode()

class Tests(unittest.TestCase):
    def test_only_scenario_values_change_and_safe_summary(self):
        a = raw(body()); b, proof = p.prepare(a)
        expected = body(); expected['input'][1]['content'][0]['text'] = p.PROMPT_B
        expected['tools'][0]['name'] = 'apple_price_lookup'
        expected['tools'][0]['description'] = p.DESCRIPTION_B
        expected['text']['format']['schema'] = p.SCHEMA_B
        self.assertEqual(json.loads(b), expected)
        self.assertNotIn(SECRET,json.dumps(proof)); self.assertEqual(proof['changed_fields'],4)
        self.assertEqual(p.PROMPT_B,'Look up the price per kilogram of apples. How much would two kilograms cost?')
        self.assertNotIn('token',p.PROMPT_B + json.dumps(expected['tools']) + json.dumps(p.SCHEMA_B))
        self.assertEqual(len(proof['changed_json_pointers']),4)
        # Untouched outer serialization and unrelated UTF8 content remain verbatim.
        for indent in [None,2,4]:
            value=body();value['metadata']={'note':'苹果🌳'}
            encoded=json.dumps(value,indent=indent,ensure_ascii=False).encode()
            updated,_=p.prepare(encoded)
            spans=p.value_spans(encoded.decode());spans_b=p.value_spans(updated.decode())
            paths=[('input',1,'content',0,'text'),('tools',0,'name'),('tools',0,'description'),('text','format','schema')]
            def mask(data,ranges):
                for start,end in sorted(ranges,reverse=True):data=data[:start]+'<CHANGED>'+data[end:]
                return data
            self.assertEqual(mask(encoded.decode(),[spans[x] for x in paths]),mask(updated.decode(),[spans_b[x] for x in paths]))
    def test_native_wrapper_and_other_tools_unchanged(self):
        value=body();value['tools'][0]['name']=p.flat_name('grok_history_probe')
        value['tools'][0]['description']='Call this function directly to invoke `grok_history_probe`. Do not invoke it through another tool.\n\n'+p.DESCRIPTION_A
        other={'type':'function','name':'unrelated','description':'unchanged','parameters':{'type':'object'}}
        value['tools'].append(other)
        updated,_=p.prepare(raw(value));updated=json.loads(updated)
        self.assertEqual(updated['tools'][0]['name'],p.flat_name('apple_price_lookup'))
        self.assertEqual(updated['tools'][1],other)
        self.assertIn('`apple_price_lookup`',updated['tools'][0]['description'])
    def test_tool_schema_and_leftover_context_guards(self):
        cases=[]
        value=body();value['tools'].append(copy.deepcopy(value['tools'][0]));cases.append(value)
        value=body();value['tools'][0]['parameters']={};cases.append(value)
        value=body();value['tools'][0]['description']='unexpected\n\n'+p.DESCRIPTION_A;cases.append(value)
        value=body();value['text']['format']['schema']={};cases.append(value)
        value=body();value['input'][0]['content'][0]['text']='grok_history_probe';cases.append(value)
        value=body();value['tools'].append({'name':'apple_price_lookup'});cases.append(value)
        for value in cases:
            with self.subTest(value=value),self.assertRaises(ValueError):p.prepare(raw(value))
    def test_escaped_leftovers_and_non_json_equivalent_schemas_rejected(self):
        for value in ['grok_history_probe',p.PROMPT_A,p.DESCRIPTION_A]:
            request=body();request['metadata']={'extra':value}
            # Encode only the metadata value to evade raw substring scanning.
            escaped='"'+''.join('\\u%04x'%ord(c) for c in value)+'"'
            # Ensure the target user task remains canonical; escape metadata explicitly.
            text=json.dumps(request,indent=2); spans=p.value_spans(text); start,end=spans[('metadata','extra')]
            encoded=(text[:start]+escaped+text[end:]).encode()
            with self.subTest(value=value),self.assertRaisesRegex(ValueError,'old_fixture_context_remains'):
                p.prepare(encoded)
        for location in ['parameters','schema']:
            request=body()
            target=request['tools'][0]['parameters'] if location=='parameters' else request['text']['format']['schema']
            target['additionalProperties']=0
            with self.subTest(location=location),self.assertRaises(ValueError):p.prepare(raw(request))

    def test_variant_cannot_overflow_request_budget(self):
        value = body(); value['padding'] = ''
        value['padding'] = 'x' * (p.MAX_BYTES - len(raw(value)))
        encoded = raw(value)
        self.assertEqual(len(encoded), p.MAX_BYTES)
        with mock.patch.object(p,'PROMPT_B','x'*1024), self.assertRaisesRegex(ValueError, 'modified_body_bounds'):
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
