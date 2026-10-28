"""Offline-only scenario transform; no transport or credential API."""
import hashlib
import json

FORMAL_SHA = 'e3c0812c7d47c567d42a181c921a30aa3c35cdc0'
PROMPT_A = 'Call grok_history_probe, reason about its returned token, then return JSON with the exact token as the token field, or null if unavailable.'
PROMPT_B = 'Look up the price per kilogram of apples. How much would two kilograms cost?'
DESCRIPTION_A = 'Return a fresh token for this history probe.'
DESCRIPTION_B = 'Return the current price of apples in yuan per kilogram.'
PARAMETERS = {'type':'object','properties':{},'additionalProperties':False}
SCHEMA_A = {'type':'object','properties':{'token':{'type':['string','null']}},'required':['token'],'additionalProperties':False}
SCHEMA_B = {'type':'object','properties':{'amount':{'type':['number','null']}},'required':['amount'],'additionalProperties':False}
MAX_BYTES = 2 * 1024 * 1024


def strict_object(pairs):
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError('duplicate_json_key')
        obj[key] = value
    return obj


def parse(raw):
    return json.loads(raw, object_pairs_hook=strict_object,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite_json')))


def value_spans(text):
    """Locate parsed JSON value ranges without reserializing surrounding bytes."""
    decoder = json.JSONDecoder()
    spans = {}
    def ws(i):
        while i < len(text) and text[i] in ' \t\r\n': i += 1
        return i
    def walk(i, path):
        i = ws(i); start = i
        if text[i] == '{':
            i = ws(i + 1)
            if text[i] != '}':
                while True:
                    key, i = decoder.raw_decode(text, i); i = ws(i)
                    if text[i] != ':': raise ValueError('json_span')
                    i = ws(walk(i + 1, path + (key,)))
                    if text[i] == '}': break
                    if text[i] != ',': raise ValueError('json_span')
                    i = ws(i + 1)
            i += 1
        elif text[i] == '[':
            i = ws(i + 1); index = 0
            if text[i] != ']':
                while True:
                    i = ws(walk(i, path + (index,))); index += 1
                    if text[i] == ']': break
                    if text[i] != ',': raise ValueError('json_span')
                    i = ws(i + 1)
            i += 1
        else:
            _, i = decoder.raw_decode(text, i)
        spans[path] = (start, i)
        return i
    end = ws(walk(0, ()))
    if end != len(text): raise ValueError('json_span')
    return spans


def flat_name(name):
    # Exact formal164 codex-rs/tools/src/flat_projection.rs function projection.
    value = 0xcbf29ce484222325
    for field in (b'function', b'', name.encode('utf-8')):
        for byte in len(field).to_bytes(8,'little') + field:
            value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f'local___{name}_{value:016x}'


def prepare(raw):
    """Change only task text, one fixture tool's name/description, and output schema."""
    if not isinstance(raw, bytes) or not 0 < len(raw) <= MAX_BYTES:
        raise ValueError('body_bounds')
    body = parse(raw)
    if not isinstance(body, dict) or not isinstance(body.get('input'), list):
        raise ValueError('request_shape')
    matches = []
    for i, item in enumerate(body['input']):
        if not isinstance(item, dict): raise ValueError('input_shape')
        if item.get('type') in ('function_call', 'function_call_output', 'reasoning'):
            raise ValueError('not_initial_request')
        content = item.get('content')
        if (item.get('type') == 'message' and item.get('role') == 'user'
                and isinstance(content, list) and len(content) == 1
                and isinstance(content[0], dict) and content[0].get('type') == 'input_text'
                and content[0].get('text') == PROMPT_A): matches.append(i)
    if len(matches) != 1: raise ValueError('unique_standalone_task_required')
    tools = body.get('tools')
    if not isinstance(tools, list): raise ValueError('tool_shape')
    candidates = [(i,t) for i,t in enumerate(tools) if isinstance(t,dict)
                  and isinstance(t.get('description'),str)
                  and (t['description'] == DESCRIPTION_A or t['description'].endswith('\n\n'+DESCRIPTION_A))]
    if len(candidates) != 1: raise ValueError('unique_fixture_tool_required')
    ti, tool = candidates[0]
    name = tool.get('name')
    if (tool.get('type') != 'function' or json.dumps(tool.get('parameters'),sort_keys=True) != json.dumps(PARAMETERS,sort_keys=True)
            or name not in ('grok_history_probe', flat_name('grok_history_probe'))):
        raise ValueError('fixture_tool_binding')
    new_name = 'apple_price_lookup' if name == 'grok_history_probe' else flat_name('apple_price_lookup')
    if any(isinstance(t,dict) and t.get('name') == new_name for i,t in enumerate(tools) if i != ti):
        raise ValueError('tool_name_collision')
    description = tool['description']
    if description == DESCRIPTION_A: new_description = DESCRIPTION_B
    else:
        prefix = 'Call this function directly to invoke `grok_history_probe`. Do not invoke it through another tool.\n\n'
        if description != prefix + DESCRIPTION_A: raise ValueError('unknown_native_wrapper')
        new_description = prefix.replace('grok_history_probe','apple_price_lookup') + DESCRIPTION_B
    try: schema = body['text']['format']['schema']
    except (KeyError,TypeError): raise ValueError('output_schema_binding')
    if json.dumps(schema,sort_keys=True) != json.dumps(SCHEMA_A,sort_keys=True): raise ValueError('output_schema_binding')
    changes = {
        ('input',matches[0],'content',0,'text'):PROMPT_B,
        ('tools',ti,'name'):new_name,
        ('tools',ti,'description'):new_description,
        ('text','format','schema'):SCHEMA_B,
    }
    text = raw.decode('utf-8'); spans = value_spans(text); expected = parse(raw)
    for path,value in changes.items():
        owner = expected
        for key in path[:-1]: owner = owner[key]
        owner[path[-1]] = value
    replacements = sorted([(spans[path],json.dumps(value,separators=(',',':'))) for path,value in changes.items()],reverse=True)
    for (start,end),value in replacements: text = text[:start] + value + text[end:]
    modified = text.encode('utf-8')
    if len(modified) > MAX_BYTES: raise ValueError('modified_body_bounds')
    if parse(modified) != expected: raise ValueError('scenario_change_proof_failed')
    # Any undeclared duplicate task/name/context must stop, not be globally replaced.
    semantic = json.dumps(expected, ensure_ascii=False)
    if 'grok_history_probe' in semantic or DESCRIPTION_A in semantic or PROMPT_A in semantic:
        raise ValueError('old_fixture_context_remains')
    return modified, {
        'formal_sha':FORMAL_SHA,'comparison':'whole_task_scenario_not_single_word',
        'changed_json_pointers':['/'+'/'.join(map(str,path)) for path in changes],
        'changed_fields':4,'all_other_wire_bytes_identical':True,
        'a_body_sha256':hashlib.sha256(raw).hexdigest(),'b_body_sha256':hashlib.sha256(modified).hexdigest(),
        'a_bytes':len(raw),'b_bytes':len(modified),'network_requests':0,
    }
