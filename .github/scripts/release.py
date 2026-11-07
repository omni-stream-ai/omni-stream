"""Version detection, commit-based release notes, and GitHub Release publishing."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

import artifacts

ROOT = Path(__file__).resolve().parents[2]
CONFIG = 'apps/gui/src-tauri/tauri.conf.json'
SEMVER = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?')


def command(*args, root=ROOT):
    return subprocess.check_output(args, cwd=root, text=True).strip()


def version(root=ROOT):
    value = json.loads((root / CONFIG).read_text())['version']
    if not SEMVER.fullmatch(value):
        raise ValueError('Use a semantic version without build metadata, such as 0.2.0 or 0.2.0-beta.1')
    prerelease = value.partition('-')[2]
    if any(part.isdigit() and len(part) > 1 and part.startswith('0') for part in prerelease.split('.')):
        raise ValueError('Numeric prerelease identifiers cannot have leading zeroes')
    manifests = {
        'Cargo.toml': tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version'],
        'apps/gui/src-tauri/Cargo.toml': tomllib.loads((root / 'apps/gui/src-tauri/Cargo.toml').read_text())['package']['version'],
        'apps/gui/package.json': json.loads((root / 'apps/gui/package.json').read_text())['version'],
    }
    mismatches = [path for path, actual in manifests.items() if actual != value]
    if mismatches:
        raise ValueError('Release version must match ' + CONFIG + ': ' + ', '.join(mismatches))
    return value


def previous_tag(tag, sha, root=ROOT):
    tags = command('git', 'tag', '--merged', sha, '--sort=-version:refname', root=root).splitlines()
    for candidate in tags:
        if candidate != tag and candidate.startswith('v') and SEMVER.fullmatch(candidate[1:]):
            if '-' in tag or '-' not in candidate:
                return candidate
    return ''


def plan(event_name, event, branch, sha, root=ROOT):
    current = version(root)
    tag = 'v' + current
    exists = subprocess.run(['git', 'rev-parse', '--verify', '--quiet', 'refs/tags/' + tag], cwd=root, capture_output=True).returncode == 0
    build = event_name == 'workflow_dispatch'
    publish = False
    if event_name == 'push':
        before = event.get('before', '')
        previous = None
        if before and before != '0' * 40:
            result = subprocess.run(['git', 'show', before + ':' + CONFIG], cwd=root, capture_output=True, text=True)
            if result.returncode == 0:
                previous = json.loads(result.stdout)['version']
            changed = command('git', 'diff', '--name-only', before, sha, '--', '.github', root=root).splitlines()
        else:
            changed = command('git', 'show', '--format=', '--name-only', sha, '--', '.github', root=root).splitlines()
        pipeline_changed = any(path.startswith(('.github/scripts/', '.github/tests/', '.github/actions/setup-build/')) or path == '.github/workflows/release.yml' for path in changed)
        changed_version = previous != current
        build = changed_version or pipeline_changed
        same_commit = exists and command('git', 'rev-parse', tag + '^{commit}', root=root) == sha
        publish = changed_version or (pipeline_changed and (not exists or same_commit))
        primary = event.get('repository', {}).get('default_branch', 'main')
        if publish and branch == primary and '-' in current:
            raise ValueError('Publish stable versions on the default branch and prereleases on other branches')
        if branch != primary and '-' not in current:
            publish = False
    if publish and exists:
        tagged_sha = command('git', 'rev-parse', tag + '^{commit}', root=root)
        if tagged_sha != sha:
            raise ValueError(f'Tag {tag} already points to another commit; use a new version')
    return {
        'version': current, 'tag': tag, 'should_build': str(build).lower(),
        'should_release': str(publish).lower(), 'prerelease': str('-' in current).lower(),
        'previous_tag': previous_tag(tag, sha, root),
        'ios_enabled': str(os.environ.get('ENABLE_IOS_RELEASE', '').lower() == 'true').lower(),
    }


def notes(tag, previous, sha, repository, include_ios, root=ROOT):
    history = (previous + '..' + sha) if previous else sha
    log = command('git', 'log', '--no-merges', '--format=%H%x09%s', history, root=root)
    sections = {title: [] for title in ['Breaking changes', 'Features', 'Fixes', 'Performance', 'Other changes']}
    server = os.environ.get('GITHUB_SERVER_URL', 'https://github.com')
    url = server + '/' + repository
    for line in log.splitlines():
        commit, subject = line.split('\t', 1)
        kind = re.match(r'^([a-z]+)(?:\([^)]*\))?(!)?:\s', subject)
        title = 'Other changes'
        if kind:
            title = 'Breaking changes' if kind[2] else {'feat': 'Features', 'fix': 'Fixes', 'perf': 'Performance'}.get(kind[1], title)
        sections[title].append(f'- {subject} ([{commit[:7]}]({url}/commit/{commit}))')
    lines = [f'# {tag}', '', '## Downloads', '',
             'Desktop installers include a local Core. Android and iOS connect to Core on a computer or server.', '',
             'Windows installers are unsigned; macOS packages use ad-hoc signing and are not notarized.',
             'Verify downloaded files using SHA256SUMS. The Web archive contains static frontend files.', '']
    if not include_ios:
        lines += ['An iOS IPA is not included: Apple signing has not been enabled for this repository.', '']
    for title, entries in sections.items():
        if entries:
            lines += ['## ' + title, '', *entries, '']
    if not log:
        lines += ['No new commits in this release range.', '']
    if previous:
        lines += [f'**Full changelog:** {url}/compare/{previous}...{tag}', '']
    return '\n'.join(lines)


def ensure_tag(tag, sha, root=ROOT):
    command('git', 'fetch', 'origin', '--tags', root=root)
    exists = subprocess.run(['git', 'rev-parse', '--verify', '--quiet', 'refs/tags/' + tag], cwd=root, capture_output=True).returncode == 0
    if exists:
        if command('git', 'rev-parse', tag + '^{commit}', root=root) != sha:
            raise ValueError(f'Tag {tag} points to another commit')
    else:
        command('git', '-c', 'user.name=github-actions[bot]', '-c', 'user.email=41898282+github-actions[bot]@users.noreply.github.com',
                'tag', '-a', tag, sha, '-m', 'Release ' + tag, root=root)
        command('git', 'push', 'origin', 'refs/tags/' + tag, root=root)


def publish():
    current = version()
    sha = os.environ['GITHUB_SHA']
    tag = 'v' + current
    repository = os.environ['GITHUB_REPOSITORY']
    include_ios = os.environ.get('ENABLE_IOS_RELEASE', '').lower() == 'true'
    assets = artifacts.collect(current, sha, include_ios)
    ensure_tag(tag, sha)
    previous = previous_tag(tag, sha)
    body = notes(tag, previous, sha, repository, include_ios)
    notes_file = ROOT / 'release-notes.md'
    notes_file.write_text(body, encoding='utf-8')
    endpoint = 'repos/' + repository + '/releases'
    result = subprocess.run(['gh', 'api', endpoint + '/tags/' + tag], capture_output=True, text=True, cwd=ROOT)
    if result.returncode == 0:
        release = json.loads(result.stdout)
        if not release['draft']:
            present = {item['name'] for item in release['assets']}
            missing = {path.name for path in assets} - present
            if missing:
                raise ValueError('Published release is missing assets; use a new version: ' + ', '.join(sorted(missing)))
            print('Release already published with the complete asset set; leaving it unchanged.')
            return
    else:
        if '404' not in result.stderr:
            raise RuntimeError('Could not inspect the existing GitHub Release: ' + result.stderr)
        args = ['gh', 'release', 'create', tag, '--verify-tag', '--draft', '--title', tag, '--notes-file', str(notes_file)]
        if '-' in current:
            args.append('--prerelease')
        subprocess.run(args, cwd=ROOT, check=True)
        release = json.loads(command('gh', 'api', endpoint + '/tags/' + tag))
    subprocess.run(['gh', 'release', 'upload', tag, *map(str, assets), '--clobber'], cwd=ROOT, check=True)
    update = {'name': tag, 'body': body, 'draft': False, 'prerelease': '-' in current,
              'make_latest': 'false' if '-' in current else 'legacy'}
    subprocess.run(['gh', 'api', '--method', 'PATCH', endpoint + '/' + str(release['id']), '--input', '-'],
                   cwd=ROOT, input=json.dumps(update), text=True, check=True, stdout=subprocess.DEVNULL)
    print('Published ' + tag + ' with all packages, release notes, manifest.json, and SHA256SUMS.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['detect', 'check', 'publish'])
    args = parser.parse_args()
    if args.command == 'detect':
        event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
        values = plan(os.environ['GITHUB_EVENT_NAME'], event, os.environ['GITHUB_REF_NAME'], os.environ['GITHUB_SHA'])
        with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
            output.write(''.join(key + '=' + value + '\n' for key, value in values.items()))
        print(json.dumps(values, indent=2))
    elif args.command == 'check':
        print('Release version: ' + version())
    else:
        publish()
