#!/usr/bin/env python3
"""SQL-only helpers for an explicitly authorized, temporary staging exporter.

No connection, secret generation, export or mutation occurs on import.
Passwords returned in SQL must travel only over a private stdin transport.
"""
import re


def ident(value):
    if not isinstance(value, str) or not value or '\0' in value:
        raise ValueError('invalid_identifier')
    return '"' + value.replace('"', '""') + '"'


def literal(value):
    if not isinstance(value, str) or '\0' in value:
        raise ValueError('invalid_literal')
    return "'" + value.replace("'", "''") + "'"


def inventory(tables, sequences, schemas):
    if len(tables) != 50 or len(sequences) != 6 or set(schemas) != {'public', 'epoch_guard'}:
        raise ValueError('unreviewed_inventory')
    if len(set(tables + sequences)) != 56:
        raise ValueError('duplicate_inventory')
    for value in tables + sequences:
        pieces = value.split('.')
        if len(pieces) != 2 or pieces[0] not in schemas or not re.fullmatch(r'[a-z_][a-z_0-9]*', pieces[1]):
            raise ValueError('unreviewed_relation')
    return ','.join('.'.join(ident(x) for x in v.split('.')) for v in tables + sequences)


def names(group, login):
    if not re.fullmatch(r'supa_recovery_export_[a-f0-9]{12}', group):
        raise ValueError('unreviewed_export_role')
    if login != group.replace('_export_', '_login_'):
        raise ValueError('unreviewed_login')
    return ident(group), ident(login)


def create_sql(group, login, password, expiry, tables, sequences, schemas):
    g, u = names(group, login)
    relations = inventory(tables, sequences, schemas)
    if not re.fullmatch(r'[A-Za-z0-9_-]{40,100}', password):
        raise ValueError('private_password_required')
    if not re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\+00:00', expiry):
        raise ValueError('bounded_expiry_required')
    return f"""BEGIN;
CREATE ROLE {g} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;
CREATE ROLE {u} LOGIN PASSWORD {literal(password)} VALID UNTIL {literal(expiry)} CONNECTION LIMIT 2 NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;
GRANT USAGE ON SCHEMA public,epoch_guard TO {g};
GRANT SELECT ON TABLE {relations} TO {g};
GRANT {g} TO {u} WITH ADMIN FALSE, INHERIT FALSE, SET TRUE;
ALTER ROLE {u} IN DATABASE supa_staging SET default_transaction_read_only=on;
ALTER ROLE {u} IN DATABASE supa_staging SET statement_timeout='30s';
ALTER ROLE {u} IN DATABASE supa_staging SET lock_timeout='10s';
ALTER ROLE {u} IN DATABASE supa_staging SET idle_in_transaction_session_timeout='120s';
ALTER ROLE {u} IN DATABASE supa_staging SET search_path=pg_catalog;
COMMIT;
"""


def cleanup_sql(group, login, tables, sequences, schemas):
    g, u = names(group, login)
    relations = inventory(tables, sequences, schemas)
    return f"""BEGIN;
REVOKE {g} FROM {u};
REVOKE SELECT ON TABLE {relations} FROM {g};
REVOKE USAGE ON SCHEMA public,epoch_guard FROM {g};
DROP ROLE {u};
DROP ROLE {g};
COMMIT;
"""
