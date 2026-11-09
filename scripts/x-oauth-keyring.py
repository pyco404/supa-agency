#!/usr/bin/python3
"""Owner-terminal enrollment and private backend IPC. No OAuth/network operations."""
import argparse
import getpass
import json
import os
import re
import resource
import signal
import stat
import subprocess
import sys
import time
import uuid
import warnings
from pathlib import Path

LABEL = 'supa-agency/x/supa_agency'
CALLBACK = 'http://127.0.0.1:37641/oauth/x/callback'
SCOPES = ['tweet.read', 'tweet.write', 'users.read']
OAUTH1_NAMESPACE = 'oauth1-supa-agency'
OAUTH1_FIELDS = ['X_API_KEY', 'X_API_SECRET', 'X_ACCESS_TOKEN', 'X_ACCESS_SECRET']
LOCAL_OWNER = '6fd863e1-745d-4a06-b68e-eea69e854a69'


def require(value):
    if not value:
        raise ValueError('x_private_configuration_rejected')


def identity(client, owner):
    require(isinstance(client, str) and re.fullmatch(r'[A-Za-z0-9_-]{1,200}', client))
    require(isinstance(owner, str) and str(uuid.UUID(owner)) == owner and uuid.UUID(owner).int != 0)


def credential(value):
    require(isinstance(value, str) and 0 < len(value) <= 4096 and all(33 <= ord(c) <= 126 for c in value))


def local_session():
    """OS-authenticated active local desktop; no certificate, token or PAM secret."""
    uid = os.getuid()
    require(uid > 0 and Path(__file__).resolve().stat().st_uid == uid)
    def properties(args):
        result = subprocess.run(['/usr/bin/loginctl', *args, '--no-pager'], stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=2,
                                env={'LANG': 'C.UTF-8'}, check=True)
        require(len(result.stdout) <= 2048)
        lines = result.stdout.decode('ascii').splitlines()
        require(all('=' in line for line in lines))
        pairs = [line.split('=', 1) for line in lines]
        require(len({p[0] for p in pairs}) == len(pairs))
        return dict(pairs)
    user = properties(['show-user', str(uid), '-p', 'Display', '-p', 'State'])
    session = user.get('Display', '')
    require(user.get('State') == 'active' and re.fullmatch(r'[A-Za-z0-9_]{1,64}', session))
    data = properties(['show-session', session, '-p', 'User', '-p', 'Active', '-p', 'Remote',
                       '-p', 'Class', '-p', 'State', '-p', 'LockedHint', '-p', 'Type'])
    require(data.get('User') == str(uid) and data.get('Active') == 'yes' and data.get('Remote') == 'no'
            and data.get('Class') == 'user' and data.get('State') == 'active'
            and data.get('LockedHint') == 'no' and data.get('Type') in ['wayland', 'x11', 'tty'])
    return {'uid': uid, 'session_id': session}


class Keyring:
    def __init__(self):
        import gi
        gi.require_version('Secret', '1')
        from gi.repository import Secret
        self.Secret = Secret
        self.schema = Secret.Schema.new('com.supaagency.XOAuth', Secret.SchemaFlags.NONE,
                                        {key: Secret.SchemaAttributeType.STRING for key in ['custody', 'client_id', 'owner', 'kind', 'version']})
        self.service = Secret.Service.get_sync(Secret.ServiceFlags.NONE, None)
        self.collection = Secret.Collection.for_alias_sync(self.service, 'default', Secret.CollectionFlags.NONE, None)
        require(self.collection is not None and not self.collection.get_locked())

    def attributes(self, client, owner, kind):
        identity(client, owner)
        require(kind in ['configuration', 'connection', 'oauth1'])
        return dict(custody=LABEL, client_id=client, owner=owner, kind=kind, version='1')

    def items(self, client, owner, kind):
        attrs = self.attributes(client, owner, kind)
        items = self.service.search_sync(self.schema, attrs, self.Secret.SearchFlags.ALL, None)
        # Never unlock, prompt, select an arbitrary duplicate or search another collection.
        require(len(items) <= 1)
        for item in items:
            require(item.get_object_path().startswith(self.collection.get_object_path() + '/'))
            actual = dict(item.get_attributes())
            require({k: v for k, v in actual.items() if k != 'xdg:schema'} == attrs)
            require(not item.get_locked())
        return items

    def put(self, client, owner, kind, body):
        require(not self.items(client, owner, kind))  # No replacement or credential rotation.
        raw = json.dumps(body, separators=(',', ':'))
        require(len(raw.encode()) <= 7000)
        value = self.Secret.Value.new(raw, len(raw.encode()), 'application/json')
        item = self.Secret.Item.create_sync(self.collection, self.schema, self.attributes(client, owner, kind),
                                           LABEL + '/' + kind, value, self.Secret.ItemCreateFlags.NONE, None)
        require(item is not None and not item.get_locked())
        require(len(self.items(client, owner, kind)) == 1)

    def get(self, client, owner, kind):
        items = self.items(client, owner, kind)
        require(len(items) == 1)
        require(items[0].load_secret_sync(None))
        value = items[0].get_secret()
        require(value is not None)
        raw = value.get_text()
        require(isinstance(raw, str) and len(raw.encode()) <= 7000)
        return json.loads(raw)


def configuration(body, client, owner):
    identity(client, owner)
    require(isinstance(body, dict) and set(body) == {'client_id', 'owner', 'callback', 'client_secret'})
    require(body['client_id'] == client and body['owner'] == owner and body['callback'] == CALLBACK)
    credential(body['client_secret'])
    return body


def enrollment(keyring, read=getpass.getpass, *, owner=None, client=None, authorize=None):
    with warnings.catch_warnings():
        warnings.simplefilter('error', getpass.GetPassWarning)
        owner = owner if owner is not None else read('Existing authorized owner UUID (hidden): ')
        client = client if client is not None else read('OAuth Client ID (hidden): ')
        secret = read('OAuth Client Secret (hidden): ')
        confirmation = read('Confirm Client Secret (hidden): ')
    identity(client, owner)
    credential(secret)
    require(secret == confirmation)
    body = configuration(dict(owner=owner, client_id=client, client_secret=secret, callback=CALLBACK), client, owner)
    if authorize is not None:
        authorize()
    keyring.put(client, owner, 'configuration', body)
    return {'stored': True, 'custody': LABEL, 'connected': False, 'publishing_enabled': False}


def oauth1_bundle(body):
    require(isinstance(body, dict) and set(body) == set(OAUTH1_FIELDS))
    for value in body.values():
        credential(value)
        require(len(value) <= 1024)
    return body


def oauth1_enrollment(keyring, read=getpass.getpass, authorize=local_session):
    authorize()
    with warnings.catch_warnings():
        warnings.simplefilter('error', getpass.GetPassWarning)
        body = oauth1_bundle({name: read(name + ' (hidden): ') for name in OAUTH1_FIELDS})
    authorize()  # A locked/changed OS session cannot finish enrollment.
    keyring.put(OAUTH1_NAMESPACE, LOCAL_OWNER, 'oauth1', body)
    retained = oauth1_bundle(keyring.get(OAUTH1_NAMESPACE, LOCAL_OWNER, 'oauth1'))
    import hmac
    require(all(hmac.compare_digest(body[name], retained[name]) for name in OAUTH1_FIELDS))
    return {'stored': True, 'storage_verified': True, 'credential_fields': OAUTH1_FIELDS,
            'auth_mode': 'oauth1', 'account_verified': False, 'publishing_enabled': False, 'external_requests': 0}


def ipc(operation, keyring, body):
    require(isinstance(body, dict))
    client, owner = body.get('client_id'), body.get('owner')
    identity(client, owner)
    if operation == 'backend-read-oauth1':
        require(set(body) == {'client_id', 'owner'} and owner == LOCAL_OWNER and client == OAUTH1_NAMESPACE)
        local_session()
        return oauth1_bundle(keyring.get(client, owner, 'oauth1'))
    if operation == 'backend-local-session':
        require(set(body) == {'client_id', 'owner'} and owner == LOCAL_OWNER)
        return local_session()
    if operation == 'backend-read':
        require(set(body) == {'client_id', 'owner'})
        return configuration(keyring.get(client, owner, 'configuration'), client, owner)
    if operation in ['backend-store-connection', 'backend-read-connection']:
        reading = operation == 'backend-read-connection'
        if reading:
            require(set(body) == {'client_id', 'owner', 'account_id'})
            requested_account = body['account_id']
            body = keyring.get(client, owner, 'connection')
            require(body.get('account_id') == requested_account)
        require(set(body) == {'client_id', 'owner', 'account_id', 'username', 'scope', 'expires_unix', 'access_token'})
        require(body['client_id'] == client and body['owner'] == owner)
        require(body['username'] == 'supa_agency' and isinstance(body['account_id'], str))
        require(re.fullmatch(r'[1-9][0-9]{0,19}', body['account_id']) and int(body['account_id']) <= 2**64 - 1)
        require(body['scope'] == SCOPES and type(body['expires_unix']) is int and int(time.time()) < body['expires_unix'] <= int(time.time()) + 7200)
        credential(body['access_token'])
        require(not body['access_token'].startswith('synthetic-'))
        require(len(keyring.items(client, owner, 'configuration')) == 1)
        if reading:
            return body  # Private backend FIFO only. Never a status/CLI/browser response.
        keyring.put(client, owner, 'connection', body)
        return {'stored': True}
    require(False)


class PrivateParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse normally echoes invalid arguments; they may be pasted secrets.
        raise ValueError('private_arguments_rejected')


def timed_out(*_):
    raise TimeoutError('private_operation_timed_out')


def main():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    signal.signal(signal.SIGALRM, timed_out)
    parser = PrivateParser(description=__doc__)
    parser.add_argument('operation', choices=['setup', 'setup-local', 'setup-oauth1', 'status-oauth1', 'status', 'backend-local-session', 'backend-read-oauth1', 'backend-read', 'backend-store-connection', 'backend-read-connection'])
    parser.add_argument('public_client_id', nargs='?')
    args = parser.parse_args()
    require((args.operation == 'setup-local') == (args.public_client_id is not None))
    # No libsecret operation before validating the private invocation channel.
    if args.operation.startswith('backend-'):
        require(stat.S_ISFIFO(os.fstat(0).st_mode) and stat.S_ISFIFO(os.fstat(1).st_mode))
        signal.alarm(5)
        raw = sys.stdin.buffer.read(7001)
        require(len(raw) <= 7000)
        body = json.loads(raw)
        result = ipc(args.operation, Keyring(), body)
    elif args.operation == 'setup-oauth1':
        require(all(os.isatty(fd) and os.fstat(fd).st_uid == os.getuid() for fd in [0, 1, 2]))
        result = oauth1_enrollment(Keyring())
    elif args.operation == 'status-oauth1':
        signal.alarm(5)
        keyring = Keyring()
        entries = keyring.items(OAUTH1_NAMESPACE, LOCAL_OWNER, 'oauth1')
        result = {'keyring_available_unlocked': True, 'configured': len(entries) == 1,
                  'auth_mode': 'oauth1', 'credential_values_read': 0, 'account_verification': 'not_checked_offline',
                  'publishing_enabled': False, 'external_requests': 0}
    elif args.operation == 'setup-local':
        require(all(os.isatty(fd) and os.fstat(fd).st_uid == os.getuid() for fd in [0, 1, 2]))
        identity(args.public_client_id, LOCAL_OWNER)
        local_session()
        result = enrollment(Keyring(), owner=LOCAL_OWNER, client=args.public_client_id, authorize=local_session)
    elif args.operation == 'setup':
        require(sys.stdin.isatty() and sys.stderr.isatty())
        result = enrollment(Keyring())
    else:
        signal.alarm(5)
        Keyring()  # Metadata only: no item search, credential read, unlock or write.
        result = {'keyring_available_unlocked': True, 'custody': LABEL, 'credentials_checked': False,
                  'connected': False, 'publishing_enabled': False}
    print(json.dumps(result, separators=(',', ':')))


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt):
        # Never print exception details, tracebacks, input, DBus messages or secrets.
        print('{"stopped":"x_private_operation_unavailable"}', file=sys.stderr)
        sys.exit(1)
