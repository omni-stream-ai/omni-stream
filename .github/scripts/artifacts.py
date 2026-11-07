"""Package build outputs and verify the complete set before publishing."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'release-assets'
DESKTOP = {
    'linux-x86_64': ('x86_64-unknown-linux-gnu', (('appimage/*.AppImage', '.AppImage'), ('deb/*.deb', '.deb'))),
    'windows-x86_64': ('x86_64-pc-windows-msvc', (('nsis/*.exe', '-setup.exe'),)),
    'macos-arm64': ('aarch64-apple-darwin', (('dmg/*.dmg', '.dmg'),)),
    'macos-x86_64': ('x86_64-apple-darwin', (('dmg/*.dmg', '.dmg'),)),
}
GROUPS = {'web', 'android', *('desktop-' + platform for platform in DESKTOP)}


def expected_names(group, signed=True):
    if group == 'web':
        return {'omnistream-web.tar.gz'}
    if group == 'ios':
        return {'omnistream-ios.ipa'}
    if group == 'android':
        label = '' if signed else '-unsigned'
        return {'omnistream-android' + label + extension for extension in ['.apk', '.aab']} | {
            'omnistream-android-' + abi + label + '.apk' for abi in ['armeabi-v7a', 'arm64-v8a', 'x86_64']}
    platform = group.removeprefix('desktop-')
    extension = '.zip' if platform.startswith('windows') else '.tar.gz'
    return {'omnistream-core-' + platform + extension} | {
        'omnistream-' + platform + suffix for _, suffix in DESKTOP[platform][1]}


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def single(directory, pattern):
    matches = sorted(path for path in directory.glob(pattern) if path.is_file())
    if len(matches) != 1:
        raise ValueError(f'Expected one {pattern} in {directory}, found {len(matches)}')
    return matches[0]


def copy_asset(source, name):
    if not source.is_file() or source.stat().st_size == 0:
        raise ValueError(f'Missing or empty build output: {source}')
    DEST.mkdir(exist_ok=True)
    target = DEST / name
    shutil.copy2(source, target)
    return target


def archive(directory, name):
    DEST.mkdir(exist_ok=True)
    output = DEST / name
    files = sorted(path for path in directory.rglob('*') if path.is_file())
    if not files:
        raise ValueError(f'Cannot archive an empty directory: {directory}')
    if name.endswith('.zip'):
        with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
            for path in files:
                bundle.write(path, path.relative_to(directory).as_posix())
    else:
        with tarfile.open(output, 'w:gz') as bundle:
            for path in files:
                bundle.add(path, arcname=path.relative_to(directory).as_posix(), recursive=False)
    return output


def record(group, files, signed=None):
    version = json.loads((ROOT / 'apps/gui/src-tauri/tauri.conf.json').read_text())['version']
    value = {
        'group': group, 'version': version, 'commit': os.environ['GITHUB_SHA'],
        'files': [{'name': path.name, 'sha256': digest(path), 'size': path.stat().st_size} for path in files],
    }
    if signed is not None:
        value['signed'] = signed
    (DEST / (group + '.manifest.json')).write_text(json.dumps(value, indent=2) + '\n')


def sidecar(target):
    suffix = '.exe' if 'windows' in target else ''
    source = ROOT / 'target' / target / 'release' / ('omnistream' + suffix)
    directory = ROOT / 'apps/gui/src-tauri/binaries'
    directory.mkdir(exist_ok=True)
    if not source.is_file():
        raise ValueError(f'Build Core before preparing the sidecar: {source}')
    shutil.copy2(source, directory / ('omnistream-' + target + suffix))


def desktop(platform):
    target, formats = DESKTOP[platform]
    suffix = '.exe' if platform.startswith('windows') else ''
    # The staging directory contains only the distributable Core executable.
    staging = ROOT / 'target' / 'release-core' / platform
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'target' / target / 'release' / ('omnistream' + suffix), staging / ('omnistream' + suffix))
    extension = '.zip' if platform.startswith('windows') else '.tar.gz'
    files = [archive(staging, 'omnistream-core-' + platform + extension)]
    bundles = ROOT / 'apps/gui/src-tauri/target' / target / 'release/bundle'
    for pattern, extension in formats:
        files.append(copy_asset(single(bundles, pattern), 'omnistream-' + platform + extension))
    record('desktop-' + platform, files)


def android():
    outputs = ROOT / 'apps/gui/src-tauri/gen/android/app/build/outputs'
    signed = os.environ.get('ANDROID_SIGNED') == 'true'
    label = '' if signed else '-unsigned'
    files = [copy_asset(single(outputs, 'apk/universal/release/*.apk'), 'omnistream-android' + label + '.apk')]
    for flavor, abi in [('arm', 'armeabi-v7a'), ('arm64', 'arm64-v8a'), ('x86_64', 'x86_64')]:
        files.append(copy_asset(single(outputs, 'apk/' + flavor + '/release/*.apk'), 'omnistream-android-' + abi + label + '.apk'))
    files.append(copy_asset(single(outputs, 'bundle/universalRelease/*.aab'), 'omnistream-android' + label + '.aab'))
    record('android', files, signed=signed)


def ios():
    source = single(ROOT / 'apps/gui/src-tauri/gen/apple/build', '**/*.ipa')
    record('ios', [copy_asset(source, 'omnistream-ios.ipa')], signed=True)


def verify_all(directory, version, commit, include_ios=False, require_signed=True):
    expected = GROUPS | ({'ios'} if include_ios else set())
    found = {}
    names = set()
    for path in sorted(directory.glob('*.manifest.json')):
        data = json.loads(path.read_text())
        group = data['group']
        if group not in expected or group in found:
            raise ValueError(f'Unexpected or duplicate artifact group: {group}')
        if data['version'] != version or data['commit'] != commit:
            raise ValueError(f'Artifact version or commit mismatch: {group}')
        if require_signed and group in {'android', 'ios'} and data.get('signed') is not True:
            raise ValueError(f'Release requires signed {group} packages')
        if {item['name'] for item in data['files']} != expected_names(group, data.get('signed', True)):
            raise ValueError(f'Artifact group has an incomplete or unexpected file set: {group}')
        for item in data['files']:
            name = item['name']
            if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', name) or name in names:
                raise ValueError(f'Unsafe or duplicate asset name: {name}')
            asset = directory / name
            if asset.is_symlink() or not asset.is_file() or asset.stat().st_size == 0 or asset.stat().st_size != item['size'] or digest(asset) != item['sha256']:
                raise ValueError(f'Artifact integrity check failed: {name}')
            names.add(name)
        found[group] = data
    if set(found) != expected:
        raise ValueError('Missing artifact groups: ' + ', '.join(sorted(expected - found.keys())))
    return found


def collect(version, commit, include_ios=False):
    groups = verify_all(DEST, version, commit, include_ios)
    files = sorted((item for group in groups.values() for item in group['files']), key=lambda item: item['name'])
    (DEST / 'manifest.json').write_text(json.dumps({'version': version, 'commit': commit, 'packages': list(groups.values())}, indent=2) + '\n')
    sums = ''.join(item['sha256'] + '  ' + item['name'] + '\n' for item in files)
    sums += digest(DEST / 'manifest.json') + '  manifest.json\n'
    (DEST / 'SHA256SUMS').write_text(sums)
    return [DEST / item['name'] for item in files] + [DEST / 'manifest.json', DEST / 'SHA256SUMS']


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['sidecar', 'desktop', 'web', 'android', 'ios'])
    parser.add_argument('target', nargs='?')
    args = parser.parse_args()
    if args.command == 'sidecar':
        sidecar(args.target)
    elif args.command == 'desktop':
        desktop(args.target)
    elif args.command == 'web':
        record('web', [archive(ROOT / 'apps/gui/dist', 'omnistream-web.tar.gz')])
    elif args.command == 'android':
        android()
    else:
        ios()
