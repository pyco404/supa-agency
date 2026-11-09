#!/usr/bin/python3
"""Disposable TLS OAuth server: fixed synthetic fixtures; never contacts X."""
import base64
import http.server
import json
import ssl
import sys
import time
from pathlib import Path
from urllib.parse import parse_qs

mode, certificate, key, stats = sys.argv[1:]
count = 0


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def handle_request(self):
        global count
        count += 1
        Path(stats).write_text(json.dumps({'calls': count, 'posting_calls': int(self.path == '/2/tweets')}))
        status = 200
        headers = {}
        if self.command == 'POST' and self.path == '/2/oauth2/token':
            expected = base64.b64encode(b'synthetic_confidential_client:fixture-secret-for-local-test').decode()
            assert self.headers['Authorization'] == 'Basic ' + expected
            assert self.headers['Content-Type'] == 'application/x-www-form-urlencoded'
            body = parse_qs(self.rfile.read(int(self.headers['Content-Length'])).decode())
            assert set(body) == {'grant_type', 'redirect_uri', 'code', 'code_verifier'}
            assert body['grant_type'] == ['authorization_code']
            assert body['redirect_uri'] == ['http://127.0.0.1:37641/oauth/x/callback']
            assert body['code'][0].startswith('synthetic-oauth-code-')
            assert len(body['code_verifier'][0]) == 43
            result = dict(access_token='fixture-access-token-only', token_type='bearer',
                          scope='tweet.read tweet.write users.read', expires_in=7200)
            if mode == 'redirect':
                status, headers = 302, {'Location': 'https://example.invalid/never-follow'}
            elif mode == 'unauthorized':
                status = 401
            elif mode == 'rate_limit':
                status = 429
            elif mode == 'scope':
                result['scope'] += ' offline.access'
            elif mode == 'refresh':
                result['refresh_token'] = 'fixture-unrequested-refresh'
            elif mode == 'disconnect':
                self.close_connection = True
                return
            elif mode == 'slow':
                time.sleep(6)
        elif self.command == 'GET' and self.path == '/2/users/me':
            assert self.headers['Authorization'] == 'Bearer fixture-access-token-only'
            result = {'data': {'id': '123', 'username': 'other' if mode in ['account', 'posting_account'] else 'supa_agency'}}
            if mode == 'partial':
                result['errors'] = [{'message': 'synthetic partial error'}]
        elif self.command == 'POST' and self.path == '/2/tweets' and mode.startswith('posting_'):
            assert self.headers['Authorization'] == 'Bearer fixture-access-token-only'
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert set(body) == {'text'} and body['text'] == 'A genuine synthetic fixture result.'
            status = {'posting_rate': 429, 'posting_credits': 402, 'posting_expired': 401,
                      'posting_server': 503, 'posting_redirect': 302}.get(mode, 201)
            headers = {'x-rate-limit-reset': '123456'}
            if mode == 'posting_redirect':
                headers['Location'] = 'https://example.invalid/never-follow'
            if mode == 'posting_disconnect':
                self.close_connection = True
                return
            result = {'data': {'id': '456', 'text': body['text']}}
        else:
            status, result = 400, {}
        raw = json.dumps(result).encode()
        if self.command == 'POST' and mode == 'oversize':
            raw = b'x'*16385
        if self.command == 'POST' and mode == 'malformed':
            raw = b'{'
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        if mode != 'oversize':
            self.send_header('Content-Length', str(len(raw)))
        for name, value in headers.items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(raw)

    do_GET = handle_request
    do_POST = handle_request


server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
if mode == 'legacy_tls':
    context.minimum_version = ssl.TLSVersion.TLSv1
    context.maximum_version = ssl.TLSVersion.TLSv1_1
    context.set_ciphers('ALL:@SECLEVEL=0')
else:
    context.minimum_version = ssl.TLSVersion.TLSv1_2
context.load_cert_chain(certificate, key)
server.socket = context.wrap_socket(server.socket, server_side=True)
print(json.dumps({'port': server.server_address[1]}), flush=True)
server.serve_forever()
