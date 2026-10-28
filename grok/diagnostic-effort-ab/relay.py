"""Single-purpose, bounded Grok effort experiment relay. No request bodies are retained."""
import http.client
import hashlib
from datetime import datetime, timezone
import json
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = 'grok.trustedtunnel.app'
PATH = '/v1/responses'
MAX_BYTES = 16 * 1024 * 1024


def safe_shape(body):
    if not isinstance(body, dict):
        return {'body_object': False}
    known = {'model', 'input', 'reasoning', 'store', 'stream', 'include', 'tools',
             'tool_choice', 'parallel_tool_calls', 'prompt_cache_key', 'text',
             'temperature', 'top_p', 'max_output_tokens', 'instructions', 'metadata'}
    types = {'message', 'reasoning', 'function_call', 'function_call_output', 'compaction'}
    items = body.get('input') if isinstance(body.get('input'), list) else []
    calls = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call']
    outputs = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call_output']
    cipher = [x['encrypted_content'] for x in items if isinstance(x, dict)
              and isinstance(x.get('encrypted_content'), str)]
    reasoning = body.get('reasoning')
    effort = reasoning.get('effort') if isinstance(reasoning, dict) else None
    text = body.get('text')
    fmt = text.get('format') if isinstance(text, dict) else None
    schema = fmt.get('schema') if isinstance(fmt, dict) else None
    return {'body_object': True, 'topkeys': sorted(set(body) & known),
            'unknown_topkey_count': len(set(body) - known),
            'model': 'grok-4.6' if body.get('model') == 'grok-4.6' else 'other',
            'effort': effort if effort in ('high', None) else 'other',
            'schema_sha256': hashlib.sha256(json.dumps(schema, sort_keys=True, separators=(',', ':')).encode()).hexdigest() if schema is not None else None,
            'input_count': len(items),
            'input_types': [x.get('type') if isinstance(x, dict) and x.get('type') in types else 'other' for x in items[:256]],
            'input_types_truncated': len(items) > 256,
            'cipher_present': bool(cipher), 'cipher_lengths': [len(x) for x in cipher[:256]],
            'cipher_lengths_truncated': len(cipher) > 256,
            'call_count': len(calls), 'output_count': len(outputs),
            'outputs_paired': bool(outputs) and all(any(c.get('call_id') == o.get('call_id') and c.get('call_id') for c in calls) for o in outputs)}


class Relay:
    def __init__(self, mode='offline', offline_transport=None):
        if mode not in ('offline', 'real'):
            raise ValueError('invalid mode')
        if mode == 'real' and offline_transport is not None:
            raise ValueError('real transport cannot be replaced')
        self.mode, self.transport = mode, offline_transport
        self.lock = threading.Lock()
        self.arms, self.active, self.total, self.events = {}, None, 0, []

    def __enter__(self):
        relay = self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_POST(self):
                status, headers, data = relay.handle(self)
                self.send_response(status)
                for key, value in headers.items():
                    self.send_header(key, value)
                self.send_header('Content-Length', str(len(data)))
                self.send_header('Connection', 'close')
                self.end_headers()
                try:
                    self.wfile.write(data)
                except (BrokenPipeError, ConnectionResetError):
                    pass
            def do_GET(self):
                relay.record_rejection('method', 405)
                self.send_error(405)
            do_PUT = do_DELETE = do_PATCH = do_HEAD = do_OPTIONS = do_GET
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f'http://127.0.0.1:{self.server.server_port}/v1'
        return self

    def __exit__(self, *args):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def begin_arm(self, label, effort):
        with self.lock:
            if (self.active is not None or label in self.arms or len(self.arms) >= 2
                    or not re.fullmatch(r'[1-4]-[AB]', label)
                    or effort not in (None, 'high')):
                raise ValueError('invalid arm transition')
            self.arms[label] = {'effort': effort, 'sent': 0, 'failed': False, 'busy': False}
            self.active = label

    def end_arm(self):
        with self.lock:
            if self.active and self.arms[self.active]['busy']:
                raise ValueError('request still active')
            self.active = None

    def summary(self):
        with self.lock:
            return json.loads(json.dumps({'mode': self.mode, 'sent': self.total,
                                         'arms': self.arms, 'events': self.events}))

    def record_rejection(self, reason, status, observed=None):
        with self.lock:
            arm = self.arms.get(self.active)
            self.events.append({**(observed or {}), 'arm': self.active,
                'utc': datetime.now(timezone.utc).isoformat(), 'state': 'rejected',
                'reason': reason, 'status': status, 'send_attempted': False,
                'phase': 'continuation' if arm and arm['sent'] else 'initial'})
        return self.reject(status)

    @staticmethod
    def reject(status=409):
        return status, {'Content-Type': 'application/json'}, b'{"error":"experiment relay rejected request"}'

    def handle(self, request):
        known_headers = {'authorization','content-type','accept','accept-encoding','content-length','host','connection','user-agent','session-id','thread-id','originator','version','x-client-request-id','x-codex-beta-features','x-codex-guardian','x-codex-installation-id','x-codex-parent-thread-id','x-codex-routing-hint','x-codex-turn-metadata','x-codex-turn-state','x-codex-window-id','x-codex-ws-stream-request-start-ms','x-openai-internal-codex-responses-lite','x-openai-memgen-request','x-openai-subagent','x-responsesapi-include-timing-metrics'}
        all_names = {name.lower() for name in request.headers}
        names = sorted(all_names & known_headers)
        observed = {'utc': datetime.now(timezone.utc).isoformat(),
                    'headers': {'incoming': names[:64], 'forwarded': [], 'dropped': names[:64],
                                'names_truncated': len(names) > 64, 'unknown_name_count':len(all_names-known_headers),
                                'invalid_name_count': sum(not (len(name) <= 64 and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9-]*', name)) for name in request.headers)}}
        # Parse locally before reserving; malformed or unrelated routes never reach upstream.
        try:
            if request.path != PATH or request.headers.get('Transfer-Encoding') is not None:
                return self.record_rejection('route_or_encoding', 400, observed)
            sizes = request.headers.get_all('Content-Length', [])
            if len(sizes) != 1 or not sizes[0].isdigit() or not 0 < int(sizes[0]) <= MAX_BYTES:
                return self.record_rejection('content_length', 400, observed)
            request.connection.settimeout(30)
            raw = request.rfile.read(int(sizes[0]))
            if len(raw) != int(sizes[0]):
                return self.record_rejection('incomplete_body', 400, observed)
            body = json.loads(raw)
            observed['shape'] = safe_shape(body)
            if not isinstance(body, dict) or body.get('model') != 'grok-4.6':
                return self.record_rejection('model', 400, observed)
            reasoning = body.get('reasoning') or {}
            if not isinstance(reasoning, dict):
                return self.record_rejection('reasoning_shape', 400, observed)
            auth = request.headers.get_all('Authorization', [])
            if len(auth) != 1 or not auth[0].startswith('Bearer '):
                return self.record_rejection('authorization_shape', 400, observed)
            headers = {'Authorization': auth[0], 'Content-Type': 'application/json', 'Accept': 'text/event-stream'}
            for name in ('User-Agent', 'session-id', 'thread-id', 'x-client-request-id', 'originator', 'version'):
                if request.headers.get(name):
                    headers[name] = request.headers[name]
            forwarded = sorted(name.lower() for name in headers)
            observed['headers']['forwarded'] = forwarded
            observed['headers']['dropped'] = [name for name in names if name not in forwarded][:64]
        except (ValueError, OSError, RecursionError, TypeError):
            return self.record_rejection('parse_or_read', 400, observed)
        with self.lock:
            label = self.active
            arm = self.arms.get(label)
            observed['phase'] = 'continuation' if arm and arm['sent'] else 'initial'
            if (arm is None or arm['busy'] or arm['failed'] or arm['sent'] >= 2 or self.total >= 4):
                self.events.append({**observed, 'arm': label, 'state': 'blocked', 'status': 409, 'send_attempted': False})
                return self.reject()
            items = body.get('input')
            outputs = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call_output'] if isinstance(items, list) else []
            calls = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call'] if isinstance(items, list) else []
            if (arm['sent'] == 0 and outputs or arm['sent'] == 1 and
                    (len(outputs) != 1 or not any(x.get('call_id') == outputs[0].get('call_id')
                                                 and x.get('call_id') for x in calls))):
                arm['failed'] = True
                self.events.append({**observed, 'arm': label, 'state': 'not_tool_continuation', 'status': 400, 'send_attempted': False})
                return self.reject(400)
            if ('effort' in reasoning if arm['effort'] is None else reasoning.get('effort') != 'high'):
                self.events.append({**observed, 'arm': label, 'state': 'effort_mismatch', 'status': 400, 'send_attempted': False})
                arm['failed'] = True
                return self.reject(400)
            # Atomic reservation includes timeouts and all failed upstream sends. Never refunded.
            self.total += 1
            arm['sent'] += 1
            arm['busy'] = True
            sequence = arm['sent']
        status, response_headers = None, {}
        try:
            if self.mode == 'offline':
                if self.transport is None:
                    raise ValueError('offline transport is required')
                status, response_headers, data = self.transport(body, headers)
            else:
                conn = http.client.HTTPSConnection(HOST, timeout=90)
                try:
                    conn.request('POST', PATH, body=raw, headers=headers)
                    response = conn.getresponse()
                    status, response_headers = response.status, dict(response.getheaders())
                    data = response.read(MAX_BYTES + 1)
                finally:
                    conn.close()
            if not isinstance(data, bytes) or len(data) > MAX_BYTES:
                raise ValueError('response budget exceeded')
            # Redirects are never followed; response bodies are relayed but never persisted.
            failed = status != 200
            for line in data.splitlines():
                if line.startswith(b'event:') and line[6:].strip() in (b'error', b'response.failed', b'response.incomplete'):
                    failed = True
                if line.startswith(b'data:'):
                    try:
                        frame = json.loads(line[5:].strip())
                    except (ValueError, UnicodeError):
                        continue
                    if isinstance(frame, dict) and (frame.get('type') in ('error', 'response.failed', 'response.incomplete')
                            or isinstance(frame.get('response'), dict) and frame['response'].get('status') in ('failed', 'incomplete')):
                        failed = True
            event = {**observed, 'completed_utc': datetime.now(timezone.utc).isoformat(), 'send_attempted': True, 'arm': label, 'request': sequence, 'status': status, 'state': 'failed' if failed else 'received'}
            safe_headers = {'Content-Type': 'text/event-stream' if status == 200 else 'application/json'}
            # Do not relay redirects or error bodies that can echo credentials/private input.
            result = (status, safe_headers, data) if status == 200 else self.reject(status if 400 <= status <= 599 else 502)
        except Exception:
            failed = True
            event = {**observed, 'completed_utc': datetime.now(timezone.utc).isoformat(), 'send_attempted': True, 'arm': label, 'request': sequence, 'status': status, 'relay_status': 502, 'state': 'transport_failed'}
            result = self.reject(502)
        finally:
            for key, value in response_headers.items():
                if key.lower() in ('x-request-id', 'request-id', 'x-grok-request-id') and isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9._:-]{1,160}', value):
                    event[key.lower()] = value
            with self.lock:
                arm['busy'] = False
                arm['failed'] |= failed
                self.events.append(event)
        return result
