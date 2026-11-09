#!/usr/bin/env python3
"""Local-only literal dotenv validation and process launch. Never sources shell code."""
import argparse
import json
import os
from pathlib import Path
import re
import resource
import stat
import sys
from urllib.parse import urlsplit
import uuid

ROOT = Path(__file__).resolve().parents[1]
MINT = 'SUPAojJGEuTrdknFuad7TSujxgU1x2Bz2ch2bZqFfqF'
CALLBACK = 'http://127.0.0.1:37641/oauth/x/callback'
CUSTODY = 'supa-agency/x/supa_agency'
OAUTH1_FIELDS = ('X_API_KEY', 'X_API_SECRET', 'X_ACCESS_TOKEN', 'X_ACCESS_SECRET')
KEY = re.compile(r'[A-Z][A-Z0-9_]*')
DISABLED = '''SUPA_ALLOW_OPENAI SUPA_ALLOW_ANTHROPIC SUPA_CONTROLLED_INFERENCE
SUPA_CONTROLLED_WORK SUPA_SUPERVISED_REAL_MODEL SUPA_WORKER_ENABLED
SUPA_API_SCHEDULER_ENABLED SUPA_WALLET_ENABLED SUPA_PRIVATE_STAGING
SUPA_PUBLIC_LANDING_ENABLED SUPA_SYNTHETIC_FACTORY_ENABLED
SUPA_AGENT_AVATARS_ENABLED SUPA_SUPERVISED_TASKS_ENABLED SUPA_SOCIAL_REVIEW_ENABLED'''.split()
WEB = set('''SUPA_SITE_MODE SUPA_SITE_ORIGIN SUPA_BACKEND_URL SUPA_API_TOKEN
SUPA_WEB_USERNAME SUPA_WEB_PASSWORD SUPA_WEB_OPERATORS PORT'''.split() + DISABLED)
BASE = {'PATH', 'HOME', 'USER', 'LANG', 'LC_ALL', 'TERM', 'CARGO_HOME', 'RUSTUP_HOME',
        'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR'}


class Invalid(ValueError):
    """Only static field names and categories are safe to report."""


def require(ok, category):
    if not ok:
        raise Invalid(category)


def parse(text):
    values = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        key, sep, value = line.partition('=')
        key, value = key.strip(), value.strip()
        require(sep and KEY.fullmatch(key) and key not in values, 'dotenv_syntax_or_duplicate')
        if value.startswith(('"', "'")):
            require(len(value) >= 2 and value[-1] == value[0], 'dotenv_quotes')
            value = value[1:-1]
        require(all(ord(c) >= 32 and ord(c) != 127 for c in value), 'dotenv_literal_required')
        values[key] = value
    return values


def private_read(path, limit=65536):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
                and info.st_size <= limit, 'private_file_requires_owned_regular_0600')
        data = source.read(limit + 1)
        require(len(data) <= limit, 'private_file_too_large')
        return data


def load(root=ROOT, shell=None):
    """Only .env, never .env.eval, .env.devnet or a deployment profile."""
    shell = os.environ if shell is None else shell
    allowed = set(parse((root / '.env.example').read_text()))
    values = parse(private_read(root / '.env').decode('utf-8'))
    require(set(values) <= allowed, 'unsupported_local_variable')
    # Reject public exports and unknown application overrides instead of silently ignoring them.
    reserved = {k for k in shell if k.startswith(('SUPA_', 'OPENAI_', 'NEXT_PUBLIC_')) or k == 'DATABASE_URL'}
    require(reserved <= allowed, 'unsupported_exported_variable')
    values.update({k: shell[k] for k in allowed if k in shell})
    return values


def oauth1_status(values):
    present = {key: bool(values.get(key)) for key in OAUTH1_FIELDS}
    invalid = [key for key in OAUTH1_FIELDS if values.get(key) and
               not (len(values[key]) <= 1024 and all(33 <= ord(c) <= 126 for c in values[key]))]
    return {'configured': all(present.values()) and not invalid, 'present': present,
            'invalid_fields': invalid, 'source': '.env', 'publishing_enabled': False,
            'external_requests': 0, 'keyring_reads': 0, 'secrets_displayed': False}


def oauth1_credentials(root=ROOT):
    # Deliberately ignore exported credentials: this path uses the private .env only.
    values = load(root, shell={})
    require(oauth1_status(values)['configured'], 'oauth1_credentials_missing_or_invalid')
    return {key: values[key] for key in OAUTH1_FIELDS}


def loopback_url(value, scheme, database=False):
    try:
        url = urlsplit(value)
        port = url.port
        return (url.scheme in scheme and url.hostname in ('127.0.0.1', '::1', 'localhost')
                and port is not None and 1024 <= port <= 65535
                and not url.fragment and not url.query
                and (bool(url.path.strip('/')) if database else url.path in ('', '/'))
                and (database or not url.username and not url.password))
    except ValueError:
        return False


def validate(values, component='all'):
    errors = []
    def check(ok, field):
        if not ok:
            errors.append(field)
    for key in DISABLED:
        check(values.get(key, 'false') == 'false', key)
    check(values.get('SUPA_SITE_MODE') == 'local', 'SUPA_SITE_MODE')
    check(values.get('SUPA_PROVIDER') == 'offline', 'SUPA_PROVIDER')
    check(values.get('SUPA_LIVE_BUDGET_MICROUSD', '0') == '0', 'SUPA_LIVE_BUDGET_MICROUSD')
    check(values.get('SUPA_MAX_OUTPUT_TOKENS') == '256', 'SUPA_MAX_OUTPUT_TOKENS')
    check(values.get('SUPA_PLATFORM_MINT') == MINT, 'SUPA_PLATFORM_MINT')
    check(values.get('SUPA_X_CALLBACK_URL') == CALLBACK, 'SUPA_X_CALLBACK_URL')
    check(values.get('SUPA_X_USERNAME') == 'supa_agency', 'SUPA_X_USERNAME')
    check(values.get('SUPA_X_CREDENTIAL_CUSTODY') == CUSTODY, 'SUPA_X_CREDENTIAL_CUSTODY')
    for key in ['SUPA_SITE_ORIGIN', 'SUPA_BACKEND_URL']:
        check(loopback_url(values.get(key, ''), ['http']), key)
    bind = values.get('SUPA_BIND', '')
    check(bool(re.fullmatch(r'127\.0\.0\.1:[0-9]{4,5}', bind))
          and 1024 <= int(bind.rsplit(':', 1)[-1]) <= 65535 if ':' in bind else False, 'SUPA_BIND')
    port = values.get('PORT', '')
    check(port.isascii() and port.isdigit() and 1024 <= int(port) <= 65535, 'PORT')
    for key in ['SUPA_INPUT_RATE_MICROUSD_PER_MILLION', 'SUPA_CACHED_RATE_MICROUSD_PER_MILLION',
                'SUPA_CACHE_WRITE_RATE_MICROUSD_PER_MILLION', 'SUPA_OUTPUT_RATE_MICROUSD_PER_MILLION']:
        rate = values.get(key, '')
        check(not rate or rate.isascii() and rate.isdigit() and int(rate) >= 0, key)
    for key in ['OPENAI_MODEL', 'OPENAI_COMPLEX_MODEL']:
        check(bool(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,99}', values.get(key, ''))), key)
    for key, kind in [('SUPA_WEB_OPERATORS', list), ('SUPA_OPERATOR_CREDENTIALS', list),
                      ('SUPA_OPERATOR_SESSION_DEADLINES', dict)]:
        try:
            check(isinstance(json.loads(values.get(key, 'null')), kind), key)
        except (ValueError, TypeError):
            check(False, key)
    if component in ('all', 'api'):
        check(loopback_url(values.get('DATABASE_URL', ''), ['postgres', 'postgresql'], True), 'DATABASE_URL')
    if component in ('all', 'api', 'web', 'preview'):
        token = values.get('SUPA_API_TOKEN', '')
        check(32 <= len(token) <= 256 and all(33 <= ord(c) <= 126 for c in token)
              and not re.search('replace|change-me|placeholder', token, re.I), 'SUPA_API_TOKEN')
    if component in ('all', 'web', 'preview'):
        check(bool(re.fullmatch(r'[A-Za-z0-9_-]{1,64}', values.get('SUPA_WEB_USERNAME', ''))), 'SUPA_WEB_USERNAME')
        password = values.get('SUPA_WEB_PASSWORD', '')
        check(24 <= len(password) <= 256 and all(33 <= ord(c) <= 126 for c in password)
              and not re.search('replace|change-me|placeholder', password, re.I), 'SUPA_WEB_PASSWORD')
    if component in ('all', 'x'):
        check(bool(re.fullmatch(r'[A-Za-z0-9_-]{1,200}', values.get('SUPA_X_CLIENT_ID', ''))), 'SUPA_X_CLIENT_ID')
        try:
            owner = values.get('SUPA_X_OWNER_ID', '')
            check(not owner or str(uuid.UUID(owner)) == owner and uuid.UUID(owner).int != 0, 'SUPA_X_OWNER_ID')
        except ValueError:
            check(False, 'SUPA_X_OWNER_ID')
    if component == 'x-oauth1':
        status = oauth1_status(values)
        errors.extend(key for key in OAUTH1_FIELDS if not status['present'][key] or key in status['invalid_fields'])
    return errors


def verify_mint(values):
    """Verify seed-derived public bytes AND embedded public half; never sign or emit key bytes."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    require(values.get('SUPA_PLATFORM_MINT') == MINT, 'mint_identity_mismatch')
    path = Path(values.get('SUPA_PLATFORM_MINT_KEYPAIR_PATH', ''))
    require(path.is_absolute() and not any(parent.is_symlink() for parent in path.parents),
            'mint_path_requires_absolute_non_symlink_path')
    raw = json.loads(private_read(path, 4096))
    require(isinstance(raw, list) and len(raw) == 64
            and all(type(v) is int and 0 <= v <= 255 for v in raw), 'mint_keypair_format')
    public = Ed25519PrivateKey.from_private_bytes(bytes(raw[:32])).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    require(public == bytes(raw[32:]), 'mint_keypair_public_half_mismatch')
    alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    number, address = int.from_bytes(public, 'big'), ''
    while number:
        number, digit = divmod(number, 58)
        address = alphabet[digit] + address
    address = '1' * (len(public) - len(public.lstrip(b'\0'))) + address
    require(address == MINT and address.startswith('SUPA'), 'mint_derived_address_mismatch')


def command(values, component, shell=None):
    shell = os.environ if shell is None else shell
    env = {k: v for k, v in shell.items() if k in BASE}
    env.update({k: v for k, v in values.items() if component != 'x' and v and (k in WEB if component != 'api' else
                    not k.startswith(('SUPA_X_', 'SUPA_PLATFORM_', 'OPENAI_')) and k not in OAUTH1_FIELDS and k != 'PORT')})
    # Preview uses explicitly exported root settings and must not load legacy web credentials.
    if component == 'preview':
        env['SUPA_SITE_ENV_FILE'] = ''
        return ['node', 'reference-web/server.mjs'], ROOT, env
    if component == 'web':
        return ['npm', 'run', 'dev', '--', '--port', values['PORT']], ROOT / 'web', env
    if component == 'api':
        return ['cargo', 'run', '--locked', '-p', 'supa-api'], ROOT, env
    # Bind check only: never loads a secret, starts consent or calls X.
    return ['cargo', 'run', '--locked', '-p', 'supa-providers', '--bin', 'supa-x-oauth-local', '--', 'check-listener'], ROOT, env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate', 'run', 'x-setup', 'x-connect', 'x-oauth1-setup', 'x-oauth1-status', 'x-oauth1-verify', 'x-first-post-prepare', 'x-first-post-ledger-check', 'x-first-post'])
    parser.add_argument('component', nargs='?', choices=['all', 'api', 'web', 'preview', 'x', 'x-oauth1', 'mint'], default='all')
    parser.add_argument('--posting-ledger', type=Path)
    parser.add_argument('--authorization-id')
    parser.add_argument('--expires-at', type=int)
    args = parser.parse_args()
    require(args.action == 'x-first-post' or (args.authorization_id is None and args.expires_at is None), 'first_post_options_only')
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    values = load()
    if args.posting_ledger is not None:
        require(args.action in ('x-first-post-ledger-check','x-first-post'), 'posting_ledger_only_for_explicit_x_command')
        path=args.posting_ledger
        require(path.is_absolute() and not any(parent.is_symlink() for parent in path.parents), 'private_ledger_absolute_path_required')
        profile=parse(private_read(path,4096).decode('utf-8'))
        require(set(profile)=={'DATABASE_URL'}, 'posting_ledger_requires_only_database_url')
        url=profile['DATABASE_URL']
        require(loopback_url(url,['postgresql'],True) and urlsplit(url).path=='/supa_x_first_post', 'dedicated_local_ledger_required')
        # Scoped copy only: never rewrite root .env or unrelated application configuration.
        values=dict(values,DATABASE_URL=url)
    if args.action in ('x-setup', 'x-connect'):
        require(args.component in ('all', 'x'), 'select_x_only_component')
        args.component = 'x'
    if args.action.startswith(('x-oauth1-', 'x-first-post')):
        require(args.component in ('all', 'x-oauth1'), 'select_x_oauth1_only_component')
        args.component = 'x-oauth1'
    if args.action in ('x-oauth1-status', 'x-oauth1-setup'):
        print(json.dumps(oauth1_status(load(shell={}))))
        if args.action == 'x-oauth1-setup':
            print('Edit the four X_* fields in .env; no enrollment is required.')
        return 0
    if args.component == 'x-oauth1':
        private_values = load(shell={})
        values.update({key: private_values.get(key, '') for key in OAUTH1_FIELDS})
    errors = validate(values, args.component)
    if errors:
        print(json.dumps({'valid': False, 'missing_or_invalid': errors}))
        return 1
    if args.component in ('all', 'mint'):
        verify_mint(values)
    if args.action == 'validate':
        print(json.dumps({'valid': True, 'component': args.component, 'secrets_displayed': False,
                          'external_requests': 0, 'keyring_reads': 0}))
        return 0
    if args.action.startswith('x-first-post'):
        binary = ROOT / 'target/work-review-validation/debug/supa-x-first-post'
        require(binary.is_file() and not binary.is_symlink(), 'build_first_post_command_first')
        env = {k:v for k,v in os.environ.items() if k in BASE}
        if args.action == 'x-first-post-prepare':
            argv = [str(binary), 'prepare']
        elif args.action == 'x-first-post-ledger-check':
            require(loopback_url(values.get('DATABASE_URL',''),['postgresql'],True), 'local_first_post_database_required')
            env['DATABASE_URL']=values['DATABASE_URL']
            argv=[str(binary),'ledger-check']
        else:
            require(all(os.isatty(fd) for fd in [0,1,2]), 'private_owner_terminal_required')
            require(args.authorization_id is not None and args.expires_at is not None, 'exact_authorization_id_and_expiry_required')
            try: require(str(uuid.UUID(args.authorization_id)) == args.authorization_id and uuid.UUID(args.authorization_id).int != 0, 'invalid_authorization_id')
            except ValueError: raise Invalid('invalid_authorization_id')
            require(loopback_url(values.get('DATABASE_URL',''), ['postgresql'], True), 'local_first_post_database_required')
            env['DATABASE_URL'] = values['DATABASE_URL']
            argv = [str(binary), 'post', args.authorization_id, str(args.expires_at), '--authorize-one-get-and-one-post-budget-025']
        os.chdir(ROOT)
        os.execve(binary, argv, env)
    if args.action.startswith('x-oauth1-'):
        if args.action != 'x-oauth1-status':
            require(all(os.isatty(fd) for fd in [0, 1, 2]), 'private_owner_terminal_required')
        binary = ROOT / 'target/work-review-validation/debug/supa-x-oauth-local'
        require(binary.is_file() and not binary.is_symlink(), 'build_local_x_cli_first')
        operation = {'x-oauth1-setup':'setup-oauth1', 'x-oauth1-status':'status-oauth1',
                     'x-oauth1-verify':'verify-oauth1'}[args.action]
        argv = [str(binary), operation]
        if operation == 'verify-oauth1': argv.append('--authorize-one-budgeted-identity-read')
        env = {k: v for k, v in os.environ.items() if k in BASE}
        os.chdir(ROOT)
        os.execve(binary, argv, env)
    if args.action in ('x-setup', 'x-connect'):
        require(all(os.isatty(fd) for fd in [0, 1, 2]), 'private_owner_terminal_required')
        binary = ROOT / 'target/work-review-validation/debug/supa-x-oauth-local'
        require(binary.is_file() and not binary.is_symlink(), 'build_local_x_cli_first')
        env = {k: v for k, v in os.environ.items() if k in BASE}
        operation = 'setup-local' if args.action == 'x-setup' else 'connect-local'
        argv = [str(binary), operation, values['SUPA_X_CLIENT_ID']]
        if args.action == 'x-connect':
            argv.append('--authorize-one-connection-and-one-budgeted-identity-read')
        os.chdir(ROOT)
        os.execve(binary, argv, env)
    require(args.component in ('api', 'web', 'preview', 'x'), 'select_explicit_run_component')
    argv, cwd, env = command(values, args.component)
    os.chdir(cwd)
    os.execvpe(argv[0], argv, env)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Invalid as error:
        print(json.dumps({'valid': False, 'error': str(error)}))
        sys.exit(1)
    except (OSError, ValueError, ImportError):
        print(json.dumps({'valid': False, 'error': 'configuration_unavailable_or_malformed'}))
        sys.exit(1)
