#!/usr/bin/python3
"""Disposable TLS OAuth server: fixed synthetic fixtures; never contacts X."""
import base64
import http.server
import hashlib
import hmac
import json
import ssl
import sys
import time
from pathlib import Path
from urllib.parse import parse_qs, unquote, quote

mode, certificate, key, stats = sys.argv[1:]
count = 0
nonces = set()

def verify_oauth1(header, method, path, port):
    assert header.startswith("OAuth ")
    fields = dict((unquote(k), unquote(v.strip('"'))) for k,v in (p.split("=",1) for p in header[6:].split(", ")))
    signature = fields.pop("oauth_signature")
    assert fields["oauth_consumer_key"] == "fixture-consumer-key"
    assert fields["oauth_token"] == "fixture-access-token"
    assert fields["oauth_signature_method"] == "HMAC-SHA1" and fields["oauth_version"] == "1.0"
    assert len(fields["oauth_nonce"]) == 32 and fields["oauth_nonce"] not in nonces
    nonces.add(fields["oauth_nonce"])
    assert abs(int(fields["oauth_timestamp"]) - int(time.time())) < 60
    encode = lambda value: quote(value, safe="~-._")
    normalized = "&".join(encode(k)+"="+encode(v) for k,v in sorted(fields.items()))
    endpoint = "https://127.0.0.1:"+str(port)+path
    base = method+"&"+encode(endpoint)+"&"+encode(normalized)
    expected = base64.b64encode(hmac.new(b"fixture-consumer-secret&fixture-access-secret", base.encode(), hashlib.sha1).digest()).decode()
    assert hmac.compare_digest(signature, expected)


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
            if mode.startswith('oauth1_'):
                verify_oauth1(self.headers['Authorization'], 'GET', self.path, self.server.server_port)
                status = {'oauth1_expired':401, 'oauth1_rate':429, 'oauth1_credits':402, 'oauth1_redirect':302}.get(mode,200)
                if mode == 'oauth1_redirect': headers['Location'] = 'https://example.invalid/never-follow'
            else:
                assert self.headers['Authorization'] == 'Bearer fixture-access-token-only'
            result = {'data': {'id': '0' if mode == 'oauth1_bad_id' else '123', 'username': 'other' if mode in ['account', 'posting_account', 'oauth1_account', 'oauth1_post_account'] else 'supa_agency'}}
            if mode in ['partial', 'oauth1_partial']:
                result['errors'] = [{'message': 'synthetic partial error'}]
        elif self.command == 'POST' and self.path == '/2/tweets' and mode.startswith('oauth1_post_'):
            verify_oauth1(self.headers['Authorization'], 'POST', self.path, self.server.server_port)
            assert self.headers['Content-Type'] == 'application/json'
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert set(body) == {'text'}
            assert body['text'] == "Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip."
            status = {'oauth1_post_permission':403, 'oauth1_post_expired':401, 'oauth1_post_rate':429,
                      'oauth1_post_credits':402, 'oauth1_post_server':503, 'oauth1_post_redirect':302}.get(mode,201)
            headers['x-rate-limit-reset'] = '123456'
            if mode == 'oauth1_post_redirect':headers['Location'] = 'https://example.invalid/never-follow'
            if mode == 'oauth1_post_disconnect':
                self.close_connection = True
                return
            if mode == 'oauth1_post_slow':time.sleep(5)
            result = {'data':{'id':'0' if mode=='oauth1_post_bad_id' else '456', 'text':body['text']}}
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
        if self.command == 'POST' and mode in ['oversize', 'oauth1_post_oversize']:
            raw = b'x'*16385
        if self.command == 'POST' and mode in ['malformed', 'oauth1_post_malformed']:
            raw = b'{'
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        if mode != 'oversize' and not (self.command == 'POST' and mode == 'oauth1_post_oversize'):
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
