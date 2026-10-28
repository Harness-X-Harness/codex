"""One explicitly enabled ABBA batch of frozen first requests; no model continuation."""
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

    def body(self):
        """Return a parsed in-memory copy for the controller's fail-closed comparison."""
        with self._lock:
            if self._raw is None:
                raise ValueError('capture unavailable')
            return json.loads(self._raw)

    def headers(self):
        """Return an in-memory copy; callers must never persist values or auth hashes."""
        with self._lock:
            if self._headers is None:
                raise ValueError('capture unavailable')
            return dict(self._headers)


_PAIR_LOCK = threading.Lock()
_pair_started = False


def run_pair(a, b, report, *, enabled=False):
    """The only send entrypoint: one process-wide, fixed ABBA batch, at most four."""
    global _pair_started
    if type(report) is not dict or report:
        raise ValueError('empty safe report required')
    if enabled is not True:
        raise ValueError('real sends require explicit enablement')
    if a is b or not isinstance(a, FrozenReplay) or not isinstance(b, FrozenReplay):
        raise ValueError('two distinct captures required')
    with _PAIR_LOCK, a._lock, b._lock:
        if _pair_started or any(x._used or x._raw is None for x in (a, b)):
            raise ValueError('frozen batch unavailable')
        auth_a = next(v for k, v in a._headers.items() if k.lower() == 'authorization')
        auth_b = next(v for k, v in b._headers.items() if k.lower() == 'authorization')
        if auth_a != auth_b:
            raise ValueError('credential mismatch')
        _pair_started = True
        a._used = b._used = True
    report.update({'sent': 0, 'request_limit': 4, 'sequence': 'ABBA', 'requests': [],
                   'tool_executions': 0, 'continuations': 0,
                   'captures': {'A': a.summary(), 'B': b.summary()}})
    for index, (label, owner) in enumerate((('A', a), ('B', b), ('B', b), ('A', a))):
        if index:
            deadline = time.monotonic() + 60
            while (remaining := deadline - time.monotonic()) > 0:
                time.sleep(remaining)
        with _PAIR_LOCK:
            report['sent'] += 1  # Reservation is never refunded, even before connect.
            send_headers = dict(owner._headers)
            send_raw = owner._raw
            item = {'request': index + 1, 'arm': label, 'body_sha256': hashlib.sha256(send_raw).hexdigest(),
                    'header_values_equal_to_frozen': send_headers == owner._headers, 'utc': datetime.now(timezone.utc).isoformat(),
                    'send_monotonic': time.monotonic(), 'status': None, 'state': 'sending'}
            item['seconds_after_previous_completion'] = (item['send_monotonic'] - report['requests'][-1]['completed_monotonic']) if index else None
            report['requests'].append(item)
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
    return report
