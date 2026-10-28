"""Single-purpose, bounded Grok effort experiment relay. No request bodies are retained."""
import http.client
import json
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = 'grok.trustedtunnel.app'
PATH = '/v1/responses'
MAX_BYTES = 16 * 1024 * 1024


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
                self.send_error(405)
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
            if (self.active is not None or label in self.arms or len(self.arms) >= 4
                    or not re.fullmatch(r'[1-4]-[AB]', label)
                    or effort != (None if label.endswith('-A') else 'high')):
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

    @staticmethod
    def reject(status=409):
        return status, {'Content-Type': 'application/json'}, b'{"error":"experiment relay rejected request"}'

    def handle(self, request):
        # Parse locally before reserving; malformed or unrelated routes never reach upstream.
        try:
            if request.path != PATH or request.headers.get('Transfer-Encoding') is not None:
                return self.reject(400)
            sizes = request.headers.get_all('Content-Length', [])
            if len(sizes) != 1 or not sizes[0].isdigit() or not 0 < int(sizes[0]) <= MAX_BYTES:
                return self.reject(400)
            request.connection.settimeout(30)
            raw = request.rfile.read(int(sizes[0]))
            if len(raw) != int(sizes[0]):
                return self.reject(400)
            body = json.loads(raw)
            if not isinstance(body, dict) or body.get('model') != 'grok-4.6':
                return self.reject(400)
            reasoning = body.get('reasoning') or {}
            if not isinstance(reasoning, dict):
                return self.reject(400)
            auth = request.headers.get_all('Authorization', [])
            if len(auth) != 1 or not auth[0].startswith('Bearer '):
                return self.reject(400)
            headers = {'Authorization': auth[0], 'Content-Type': 'application/json', 'Accept': 'text/event-stream'}
            for name in ('User-Agent', 'session-id', 'thread-id', 'x-client-request-id', 'originator', 'version'):
                if request.headers.get(name):
                    headers[name] = request.headers[name]
        except (ValueError, OSError):
            return self.reject(400)
        with self.lock:
            label = self.active
            arm = self.arms.get(label)
            if (arm is None or arm['busy'] or arm['failed'] or arm['sent'] >= 2 or self.total >= 8):
                self.events.append({'arm': label, 'state': 'blocked'})
                return self.reject()
            items = body.get('input')
            outputs = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call_output'] if isinstance(items, list) else []
            calls = [x for x in items if isinstance(x, dict) and x.get('type') == 'function_call'] if isinstance(items, list) else []
            if (arm['sent'] == 0 and outputs or arm['sent'] == 1 and
                    (len(outputs) != 1 or not any(x.get('call_id') == outputs[0].get('call_id')
                                                 and x.get('call_id') for x in calls))):
                arm['failed'] = True
                self.events.append({'arm': label, 'state': 'not_tool_continuation'})
                return self.reject(400)
            if ('effort' in reasoning if arm['effort'] is None else reasoning.get('effort') != 'high'):
                self.events.append({'arm': label, 'state': 'effort_mismatch'})
                arm['failed'] = True
                return self.reject(400)
            # Atomic reservation includes timeouts and all failed upstream sends. Never refunded.
            self.total += 1
            arm['sent'] += 1
            arm['busy'] = True
            sequence = arm['sent']
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
            event = {'arm': label, 'request': sequence, 'status': status, 'state': 'failed' if failed else 'received'}
            for key, value in response_headers.items():
                if key.lower() in ('x-request-id', 'request-id', 'x-grok-request-id') and re.fullmatch(r'[A-Za-z0-9._:-]{1,160}', value):
                    event[key.lower()] = value
            safe_headers = {'Content-Type': 'text/event-stream' if status == 200 else 'application/json'}
            # Do not relay redirects or error bodies that can echo credentials/private input.
            result = (status, safe_headers, data) if status == 200 else self.reject(status if 400 <= status <= 599 else 502)
        except Exception:
            failed = True
            event = {'arm': label, 'request': sequence, 'state': 'transport_failed'}
            result = self.reject(502)
        finally:
            with self.lock:
                arm['busy'] = False
                arm['failed'] |= failed
                self.events.append(event)
        return result
