"""One explicitly enabled MFM batch of initial requests; no model continuation."""
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


_BATCH_LOCK = threading.Lock()
_batch_started = False


def run_mfm(full, report, *, enabled=False):
    """The only send entrypoint: one process-wide, fixed MFM batch, at most three."""
    global _batch_started
    if type(report) is not dict or report:
        raise ValueError('empty safe report required')
    if enabled is not True:
        raise ValueError('real sends require explicit enablement')
    if not isinstance(full, FrozenReplay):
        raise ValueError('native full capture required')
    with _BATCH_LOCK, full._lock:
        if _batch_started or full._used or full._raw is None:
            raise ValueError('frozen batch unavailable')
        auth = next(v for k, v in full._headers.items() if k.lower() == 'authorization')
        _batch_started = True
        full._used = True
    minimal_body = {'model': 'grok-4.6', 'input': [{'type': 'message', 'role': 'user',
        'content': [{'type': 'input_text', 'text': 'Reply exactly OK.'}]}],
        'stream': True, 'store': False}
    minimal_raw = json.dumps(minimal_body, separators=(',', ':')).encode()
    minimal_headers = {'Authorization': auth, 'Content-Type': 'application/json', 'Accept': 'text/event-stream'}
    report.update({'sent': 0, 'request_limit': 3, 'sequence': 'MFM', 'requests': [],
                   'tool_executions': 0, 'continuations': 0,
                   'captures': {'F': full.summary(), 'M': {'body_sha256': hashlib.sha256(minimal_raw).hexdigest(),
                       'shape': safe_shape(minimal_body), 'header_names': sorted(k.lower() for k in minimal_headers)}}})
    for index, label in enumerate(('M', 'F', 'M')):
        if index:
            deadline = time.monotonic() + 60
            while (remaining := deadline - time.monotonic()) > 0:
                time.sleep(remaining)
        with _BATCH_LOCK:
            report['sent'] += 1  # Reservation is never refunded, even before connect.
            send_headers = dict(minimal_headers if label == 'M' else full._headers)
            send_raw = minimal_raw if label == 'M' else full._raw
            item = {'request': index + 1, 'arm': label, 'body_sha256': hashlib.sha256(send_raw).hexdigest(),
                    'header_names': sorted(k.lower() for k in send_headers),
                    'header_values_equal_to_frozen': send_headers == (minimal_headers if label == 'M' else full._headers), 'utc': datetime.now(timezone.utc).isoformat(),
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
            completed, failed, incomplete = False, False, False
            public_messages = {}
            def remember_message(message, fallback):
                if label != 'M' or not isinstance(message, dict) or message.get('type') != 'message' or message.get('role') != 'assistant':
                    return
                identity = message.get('id')
                key = ('id', identity) if isinstance(identity, str) else ('index', fallback)
                text, length = '', 0
                content = message.get('content')
                for part in content if isinstance(content, list) else []:
                    if isinstance(part, dict) and part.get('type') == 'output_text' and isinstance(part.get('text'), str):
                        length += len(part['text'])
                        text += part['text'][:max(0, 256 - len(text))]
                public_messages[key] = (text, length)
            for line in data.splitlines() if response.status == 200 else []:
                if line.startswith(b'event:'):
                    name = line[6:].strip().decode('ascii', 'replace')
                    types.add(name if name in EVENT_TYPES else 'other')
                    failed |= name in ('error', 'response.failed')
                    incomplete |= name == 'response.incomplete'
                if not line.startswith(b'data:'):
                    continue
                try:
                    frame = json.loads(line[5:])
                except (ValueError, UnicodeError):
                    continue
                if not isinstance(frame, dict):
                    continue
                name = frame.get('type')
                if isinstance(name, str):
                    types.add(name if name in EVENT_TYPES else 'other')
                failed |= name in ('error', 'response.failed')
                incomplete |= name == 'response.incomplete'
                response_body = frame.get('response')
                if isinstance(response_body, dict):
                    failed |= response_body.get('status') == 'failed'
                    incomplete |= response_body.get('status') == 'incomplete'
                if name == 'response.output_item.done':
                    remember_message(frame.get('item'), frame.get('output_index', 0))
                if name == 'response.completed':
                    completed = True
                    if isinstance(response_body, dict) and isinstance(response_body.get('output'), list):
                        # Terminal output is the complete authoritative message inventory.
                        # Replace the done-message projection, so identical items are not duplicated.
                        public_messages.clear()
                        for output_index, message in enumerate(response_body['output']):
                            remember_message(message, output_index)
            item['sse_types'] = sorted(types)
            item['completion'] = 'failed' if failed else 'incomplete' if incomplete else 'completed' if completed else 'unknown'
            if label == 'M':
                item['public_output_text'] = ''.join(text for text, _ in public_messages.values())[:256]
                item['public_output_truncated'] = sum(length for _, length in public_messages.values()) > 256
            item['state'] = 'received' if response.status in (200, 403) else 'stopped_status'
            if response.status == 200 and item['completion'] == 'unknown':
                item['state'] = 'response_unknown'
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
