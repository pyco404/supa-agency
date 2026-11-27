#!/usr/bin/env python3
"""Enforce one-file commits and explicitly approved, destination-scoped pushes."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
GIT = [str(ROOT / 'scripts/git-local')]
CONVENTIONAL = re.compile(r'^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-zA-Z0-9_.:/-]+\))?!?: \S.{7,}$')
OFFICIAL_REMOTE = 'https://github.com/pyco404/supa-agency'
RELEASE_REF = 'refs/heads/release/x-pilot-validation-20261121'
RELEASE_SHA = 'f42d1484c2a9987172265ccdd635447770b25ccf'
RELEASE_BASE = '8a1c690d615c6c0025daaa3ef9ec013f133a4f5b'
RELEASE_TREE = 'bbd071389894d194300b6e074fd709b1862db91f'
MAIN_REFSPEC = 'refs/heads/main:refs/heads/main'
RELEASE_REFSPEC = RELEASE_REF + ':' + RELEASE_REF
ZERO_SHA = '0' * 40
PRODUCT_BRANCH = 'release/product-staging-20261127'
PRODUCT_BASE = '977ffa48417015a64c2b9c5f32b438c3f131f20f'
PRODUCT_TREE = 'e8112c94136b79ffc91c71b2fa9f8c507f3d6e07'
PRODUCT_ROOT = Path('/home/pikoo/Desktop/supa-agent/artifacts/phase8-product-release-20261127/application-release')
PRODUCT_REFSPEC = 'refs/heads/'+PRODUCT_BRANCH+':refs/heads/'+PRODUCT_BRANCH
PRODUCT_GATES = ('rust', 'frontend', 'typescript', 'lint', 'browser_desktop', 'browser_mobile', 'consent_denial', 'secrets', 'dependencies', 'history', 'staging_compatibility')


def product_context():
    return ROOT == PRODUCT_ROOT and ROOT.resolve() == ROOT


def product_identity():
    if git('symbolic-ref', '--short', 'HEAD').decode().strip() != PRODUCT_BRANCH:
        raise ValueError('exact_product_branch_required')
    if git('rev-parse', 'main').decode().strip() != PRODUCT_BASE:
        raise ValueError('isolated_main_must_remain_pinned')
    if git('rev-parse', PRODUCT_BASE+'^{tree}').decode().strip() != PRODUCT_TREE or not ancestor(PRODUCT_BASE, 'HEAD'):
        raise ValueError('product_base_mismatch')
    if git('rev-list', '--merges', PRODUCT_BASE+'..HEAD').strip():
        raise ValueError('product_merges_forbidden')
    if git('config', '--get', 'core.hooksPath').decode().strip() != str(ROOT/'.githooks'):
        raise ValueError('checkout_local_hooks_required')


def product_push_check():
    product_identity()
    head = git('rev-parse', 'HEAD').decode().strip()
    if os.environ.get('SUPA_APPROVED_PUSH_HEAD') != head or git('status', '--porcelain').strip():
        raise ValueError('product_requires_clean_reviewed_head')
    for mode in ((), ('--push',)):
        if git('remote', 'get-url', *mode, '--all', 'origin').decode().splitlines() != [OFFICIAL_REMOTE]:
            raise ValueError('official_origin_required')
    p = ROOT/'.git-recovery/product-release-validation.json'
    if p.is_symlink() or not p.is_file() or p.stat().st_uid != os.getuid() or p.stat().st_mode & 0o777 != 0o600:
        raise ValueError('private_validation_receipt_required')
    r = json.loads(p.read_text())
    if r.get('head') != head or r.get('base') != PRODUCT_BASE or r.get('tree') != git('rev-parse', 'HEAD^{tree}').decode().strip() or any(r.get('checks', {}).get(k) is not True for k in PRODUCT_GATES):
        raise ValueError('validated_product_release_required')
    return head


def validate_refspec(refspec):
    if product_context():
        if refspec != PRODUCT_REFSPEC:
            raise ValueError('only_matching_product_release_allowed')
        return
    if refspec not in (MAIN_REFSPEC, RELEASE_REFSPEC):
        raise ValueError('push_requires_exact_allowlisted_refspec')


def validate_release_scope(head, approved_head, dirty, fetch_urls, push_urls):
    if head != RELEASE_SHA or approved_head != RELEASE_SHA:
        raise ValueError('release_requires_exact_reviewed_sha_and_explicit_approval')
    if dirty:
        raise ValueError('push_requires_clean_working_tree')
    if fetch_urls != [OFFICIAL_REMOTE] or push_urls != [OFFICIAL_REMOTE]:
        raise ValueError('push_requires_only_the_official_origin')


def ancestor(previous, head):
    return subprocess.run(GIT + ['merge-base', '--is-ancestor', previous, head],
                          cwd=ROOT, capture_output=True).returncode == 0


def release_identity_check():
    if git('rev-parse', RELEASE_REF + '^{tree}').decode().strip() != RELEASE_TREE:
        raise ValueError('release_tree_mismatch')
    if not ancestor(RELEASE_BASE, RELEASE_SHA):
        raise ValueError('release_base_mismatch')
    if git('rev-list', '--count', RELEASE_BASE + '..' + RELEASE_SHA).strip() != b'23':
        raise ValueError('release_ancestry_count_mismatch')


def release_history_check():
    # Audit the entire release ancestry, independently of tooling's main history.
    for commit in git('rev-list', '--reverse', RELEASE_REF).decode().splitlines():
        paths = [p for p in git('diff-tree', '--root', '--no-commit-id', '--name-only',
                               '--no-renames', '-r', '-z', commit).split(b'\0') if p]
        subject = git('show', '-s', '--format=%s', commit).decode().strip()
        validate('main', paths, subject)



def git(*args):
    result = subprocess.run(GIT + list(args), cwd=ROOT, capture_output=True)
    if result.returncode:
        raise ValueError('git_policy_read_failed')
    return result.stdout


def validate(branch, paths, subject=None):
    if branch != (PRODUCT_BRANCH if product_context() else 'main'):
        raise ValueError('commits_require_main_or_scoped_product_branch')
    if len(paths) != 1:
        raise ValueError('commit_requires_exactly_one_changed_file')
    if subject is not None and not CONVENTIONAL.fullmatch(subject):
        raise ValueError('descriptive_conventional_commit_required')


def staged():
    return [p for p in git('diff', '--cached', '--name-only', '--no-renames', '-z').split(b'\0') if p]


def pre_commit():
    if product_context():
        product_identity()
    validate(git('symbolic-ref', '--short', 'HEAD').decode().strip(), staged())
    # Never bypass the existing full staged-content/reachable-history secrets gate.
    subprocess.run([sys.executable, str(ROOT / 'scripts/scan-secrets.py')], cwd=ROOT, check=True)


def commit_msg(path):
    text = Path(path).read_text()
    subject = text.splitlines()[0] if text.splitlines() else ''
    validate(git('symbolic-ref', '--short', 'HEAD').decode().strip(), staged(), subject)


def audit():
    failures = []
    audit_ref = PRODUCT_BRANCH if product_context() else 'main'
    commits = git('rev-list', '--reverse', audit_ref).decode().splitlines()
    for commit in commits:
        paths = [p for p in git('diff-tree', '--root', '--no-commit-id', '--name-only', '--no-renames', '-r', '-z', commit).split(b'\0') if p]
        subject = git('show', '-s', '--format=%s', commit).decode().strip()
        try:
            validate(audit_ref, paths, subject)
        except ValueError as error:
            failures.append({'commit': commit, 'changed_files': len(paths), 'rule': str(error)})
    report = {'branch': git('symbolic-ref', '--short', 'HEAD').decode().strip(), 'main_commits': len(commits), 'one_file_commits': len(commits) - len(failures), 'failures': failures, 'passed': not failures}
    if report['branch'] != audit_ref:
        report['passed'] = False
    print(json.dumps(report, indent=2))
    if not report['passed']:
        raise SystemExit(1)


def validate_push_scope(branch, head, approved_head, dirty, fetch_urls, push_urls):
    if branch != 'main' or not approved_head or approved_head != head:
        raise ValueError('push_requires_explicit_approval_for_current_main_head')
    if dirty:
        raise ValueError('push_requires_clean_working_tree')
    if fetch_urls != [OFFICIAL_REMOTE] or push_urls != [OFFICIAL_REMOTE]:
        raise ValueError('push_requires_only_the_official_origin')


def push_check(refspec=MAIN_REFSPEC):
    validate_refspec(refspec)
    if product_context():
        return product_push_check()
    release = refspec == RELEASE_REFSPEC
    head = git('rev-parse', RELEASE_REF if release else 'HEAD').decode().strip()
    fetch_urls = git('remote', 'get-url', '--all', 'origin').decode().splitlines()
    push_urls = git('remote', 'get-url', '--push', '--all', 'origin').decode().splitlines()
    if release:
        if git('symbolic-ref', '--short', 'HEAD').strip() != b'main':
            raise ValueError('release_push_requires_main_policy_worktree')
        validate_release_scope(head, os.environ.get('SUPA_APPROVED_PUSH_HEAD'),
                               bool(git('status', '--porcelain')), fetch_urls, push_urls)
        release_identity_check()
    else:
        validate_push_scope(git('symbolic-ref', '--short', 'HEAD').decode().strip(), head,
                            os.environ.get('SUPA_APPROVED_PUSH_HEAD'),
                            bool(git('status', '--porcelain')), fetch_urls, push_urls)
    return head


def validate_push_updates(lines, head, refspec=MAIN_REFSPEC):
    validate_refspec(refspec)
    if product_context():
        expected = 'refs/heads/'+PRODUCT_BRANCH
        if lines != [expected+' '+head+' '+expected+' '+ZERO_SHA] or head == ZERO_SHA:
            raise ValueError('product_new_branch_creation_only')
        return ZERO_SHA
    expected_ref = refspec.split(':')[0]
    release = refspec == RELEASE_REFSPEC
    if len(lines) != 1:
        raise ValueError('push_requires_exactly_one_main_update')
    fields = lines[0].split()
    if (len(fields) != 4 or fields[0] != expected_ref
            or fields[1] != head or fields[2] != expected_ref
            or not re.fullmatch(r'[0-9a-f]{40}', fields[3])
            or (fields[3] == ZERO_SHA and not release)):
        raise ValueError('push_requires_current_main_to_existing_remote_main')
    if release and head != RELEASE_SHA:
        raise ValueError('release_requires_exact_reviewed_sha')
    return fields[3]


def pre_push(remote, url):
    lines = sys.stdin.read().splitlines()
    if len(lines) != 1 or len(lines[0].split()) != 4:
        raise ValueError('push_requires_exactly_one_well_formed_update')
    fields = lines[0].split()
    refspec = fields[0] + ':' + fields[2]
    validate_refspec(refspec)
    head = push_check(refspec)
    if remote != 'origin' or url != OFFICIAL_REMOTE:
        raise ValueError('push_requires_only_the_official_origin')
    previous = validate_push_updates(lines, head, refspec)
    if previous != ZERO_SHA and not ancestor(previous, head):
        raise ValueError('remote_changed_fetch_and_review_before_push')
    audit()
    if refspec == RELEASE_REFSPEC:
        release_history_check()
    subprocess.run([sys.executable, str(ROOT / 'scripts/scan-secrets.py')], cwd=ROOT, check=True)
    if push_check(refspec) != head:
        raise ValueError('source_changed_during_push_checks')


def self_test():
    count = 0
    validate('main', [b'crates/domain/src/wallet.rs'], 'fix(wallet): validate the full Devnet genesis hash')
    count += 1
    for branch, paths, message in [
        ('feature', [b'a'], 'feat(test): reject work outside main'),
        ('main', [], 'chore(test): reject empty commits'),
        ('main', [b'a', b'b'], 'feat(test): reject bundled files'),
        ('main', [b'a'], 'update file'),
        ('main', [b'a'], 'fix: x'),
    ]:
        try:
            validate(branch, paths, message)
        except ValueError:
            count += 1
        else:
            raise AssertionError('unsafe_workflow_accepted')
    head = 'a' * 40
    official = [OFFICIAL_REMOTE]
    validate_push_scope('main', head, head, False, official, official)
    count += 1
    for values in [
        ('main', head, None, False, official, official),
        ('main', head, 'b' * 40, False, official, official),
        ('feature', head, head, False, official, official),
        ('main', head, head, True, official, official),
        ('main', head, head, False, ['https://unexpected.invalid/repo'], official),
        ('main', head, head, False, official, official * 2),
        ('main', head, head, False, ['https://github.com/pyco404/SupaAgent'], official),
        ('main', head, head, False, official, ['https://github.com/pyco404/SupaAgent']),
    ]:
        try:
            validate_push_scope(*values)
        except ValueError:
            count += 1
        else:
            raise AssertionError('unsafe_push_scope_accepted')
    update = f'refs/heads/main {head} refs/heads/main ' + 'b' * 40
    validate_push_updates([update], head)
    count += 1
    for lines in [[], [update, update], [update.replace('refs/heads/main', 'refs/heads/archive')],
                  [update.replace(head, 'c' * 40)], [update.replace('b' * 40, '0' * 40)], ['malformed']]:
        try:
            validate_push_updates(lines, head)
        except ValueError:
            count += 1
        else:
            raise AssertionError('unsafe_push_update_accepted')
    print(json.dumps({'policy_self_tests_passed': count}))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['pre-commit', 'commit-msg', 'pre-push', 'push-check', 'audit', 'self-test'])
    parser.add_argument('message_file', nargs='?')
    parser.add_argument('remote_url', nargs='?')
    args = parser.parse_args()
    try:
        if args.action == 'pre-commit':
            pre_commit()
        elif args.action == 'commit-msg':
            if not args.message_file:
                raise ValueError('message_file_required')
            commit_msg(args.message_file)
        elif args.action == 'pre-push':
            pre_push(args.message_file, args.remote_url)
        elif args.action == 'push-check':
            push_check(args.message_file or MAIN_REFSPEC)
        elif args.action == 'audit':
            audit()
        else:
            self_test()
    except (ValueError, subprocess.CalledProcessError) as error:
        code = str(error) if isinstance(error, ValueError) else 'secrets_scan_failed'
        print(json.dumps({'git_policy_passed': False, 'reason': code}), file=sys.stderr)
        raise SystemExit(1)


if __name__ == '__main__':
    main()
