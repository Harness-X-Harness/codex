"""Two explicitly enabled sends of one frozen first request; no model continuation."""
import hashlib
import http.client
import json
import re
import threading
import time
from datetime import datetime, timezone
from relay import HOST, PATH, MAX_BYTES, safe_shape

HEADER_NAMES = frozenset(('authorization', 'content-type', 'accept', 'user-agent',
    'session-id', 'thread-id', 'x-client-request-id', 'originator', 'version',
    'x-codex-beta-features', 'x-codex-turn-metadata', 'x-codex-window-id'))
EVENT_TYPES = frozenset(('response.created', 'response.in_progress', 'response.completed',
    'response.failed', 'response.incomplete', 'error', 'response.output_item.added',
    'response.output_item.done', 'response.output_text.delta', 'response.output_text.done',
    'response.function_call_arguments.delta', 'response.function_call_arguments.done',
    'response.reasoning_summary_text.delta', 'response.reasoning_summary_text.done',
    'response.content_part.added', 'response.content_part.done'))


class FrozenReplay:
    def __init__(self):
        self._lock = threading.Lock()
        self._raw = None
        self._headers = None
        self._used = False
        self._report = {'sent': 0, 'requests': []}

    def capture(self, raw, headers):
        """Offline Relay callback; retain only this initial request in process memory."""
        if not isinstance(raw, bytes) or not 0 < len(raw) <= MAX_BYTES:
            raise ValueError('invalid frozen request size')
        body = json.loads(raw)
        if (not isinstance(body, dict) or body.get('model') != 'grok-4.6'
                or body.get('store') is not False or body.get('stream') is not True
                or not isinstance(body.get('input'), list)
                or not isinstance(body.get('reasoning'), dict) or body['reasoning'].get('effort') != 'high'
                or any(isinstance(x, dict) and (x.get('type') in ('function_call', 'function_call_output')
                       or x.get('encrypted_content') is not None) for x in body['input'])):
            raise ValueError('not a store-disabled initial request')
        if (not isinstance(headers, dict) or not headers
                or any(not isinstance(k, str) or k.lower() not in HEADER_NAMES or not isinstance(v, str)
                       or '\r' in v or '\n' in v for k, v in headers.items())
                or len({k.lower() for k in headers}) != len(headers)
                or not any(k.lower() == 'authorization' and v.startswith('Bearer ') for k, v in headers.items())):
            raise ValueError('invalid frozen headers')
        with self._lock:
            if self._raw is not None or self._used:
                raise ValueError('capture already closed')
            self._raw, self._headers = raw, dict(headers)
            self._report.update(body_sha256=hashlib.sha256(raw).hexdigest(), body_bytes=len(raw),
                                shape=safe_shape(body), header_names=sorted(k.lower() for k in headers))

    def summary(self):
        with self._lock:
            return json.loads(json.dumps(self._report))

    def run(self, *, enabled=False):
        if enabled is not True:
            raise ValueError('real sends require explicit enablement')
        with self._lock:
            if self._used or self._raw is None:
                raise ValueError('frozen replay unavailable')
            self._used = True
        for index in range(2):
            if index:
                deadline = time.monotonic() + 60
                while (remaining := deadline - time.monotonic()) > 0:
                    time.sleep(remaining)
            with self._lock:
                self._report['sent'] += 1  # Reservation is never refunded, even before connect.
                send_headers = dict(self._headers)
                send_raw = self._raw
                item = {'request': index + 1, 'body_sha256': hashlib.sha256(send_raw).hexdigest(),
                        'header_values_equal_to_frozen': send_headers == self._headers, 'utc': datetime.now(timezone.utc).isoformat(),
                        'send_monotonic': time.monotonic(), 'status': None, 'state': 'sending'}
                item['seconds_after_previous_completion'] = (item['send_monotonic'] - self._report['requests'][-1]['completed_monotonic']) if index else None
                self._report['requests'].append(item)
            conn = None
            try:
                conn = http.client.HTTPSConnection(HOST, timeout=90)
                conn.request('POST', PATH, body=send_raw, headers=send_headers)
                response = conn.getresponse()
                item['status'] = response.status
                for key, value in response.getheaders():
                    if key.lower() in ('x-request-id', 'request-id', 'x-grok-request-id') and re.fullmatch(r'[A-Za-z0-9._:-]{1,160}', value):
                        item[key.lower()] = value
                data = response.read(MAX_BYTES + 1)
                if len(data) > MAX_BYTES:
                    raise ValueError('response size budget')
                types = set()
                for line in data.splitlines():
                    if line.startswith(b'event:'):
                        name = line[6:].strip().decode('ascii', 'replace')
                        types.add(name if name in EVENT_TYPES else 'other')
                    if line.startswith(b'data:'):
                        try:
                            frame = json.loads(line[5:])
                        except (ValueError, UnicodeError):
                            continue
                        if isinstance(frame, dict) and isinstance(frame.get('type'), str):
                            name = frame['type']
                            types.add(name if name in EVENT_TYPES else 'other')
                item['sse_types'] = sorted(types)
                item['state'] = 'received' if response.status in (200, 403) else 'stopped_status'
            except Exception:
                item['state'] = 'transport_unknown'
            finally:
                if conn is not None:
                    try:
                        conn.close()
                    except Exception:
                        item['state'] = 'transport_unknown'
                item['completed_utc'] = datetime.now(timezone.utc).isoformat()
                item['completed_monotonic'] = time.monotonic()
            if item['state'] != 'received':
                break
        return self.summary()
