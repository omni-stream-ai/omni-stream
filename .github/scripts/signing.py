"""Restore CI signing files from GitHub secrets without logging their contents."""

import argparse
import base64
import fnmatch
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import shlex
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def required(names):
    missing = [name for name in names if not os.environ.get(name)]
    if missing:
        raise ValueError('Configure the repository signing secrets: ' + ', '.join(missing))


def decode(value):
    return base64.b64decode(re.sub(r'\s+', '', value), validate=True)


def private_file(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(value)
    path.chmod(0o600)


def secret_command(*args):
    result = subprocess.run(args, capture_output=True)
    if result.returncode:
        raise ValueError(Path(args[0]).name + ' failed while configuring signing; verify the signing secrets')
    return result.stdout


def export(name, value):
    if '\n' in value or '\r' in value:
        raise ValueError('Unexpected newline in signing metadata: ' + name)
    with Path(os.environ['GITHUB_ENV']).open('a') as output:
        output.write(name + '=' + value + '\n')


def property_value(value):
    escaped = {'\\': '\\\\', '\n': '\\n', '\r': '\\r', '\t': '\\t', '\f': '\\f', '=': '\\=', ':': '\\:', ' ': '\\ '}
    # java.util.Properties.load(InputStream) reads Latin-1, so encode Unicode
    # as UTF-16 escapes (including surrogate pairs for non-BMP characters).
    units = value.encode('utf-16-be')
    return ''.join(escaped.get(chr(code), chr(code) if code < 128 else '\\u' + format(code, '04x'))
                   for code in (int.from_bytes(units[index:index + 2], 'big') for index in range(0, len(units), 2)))


def verify_android():
    from artifacts import single

    outputs = ROOT / 'apps/gui/src-tauri/gen/android/app/build/outputs'
    tools = os.environ.get('ANDROID_BUILD_TOOLS_VERSION', '36.0.0')
    apksigner = Path(os.environ['ANDROID_HOME']) / 'build-tools' / tools / 'apksigner'
    for flavor in ['universal', 'arm', 'arm64', 'x86_64']:
        subprocess.run([str(apksigner), 'verify', str(single(outputs, 'apk/' + flavor + '/release/*.apk'))], check=True)
    bundle = single(outputs, 'bundle/universalRelease/*.aab')
    result = subprocess.run(['jarsigner', '-verify', str(bundle)], capture_output=True, text=True, check=True)
    if 'jar verified.' not in result.stdout:
        raise ValueError('The Android App Bundle signature could not be verified')
    print('Verified universal/split APK signatures and the AAB signature.')


def android():
    names = ['ANDROID_KEYSTORE_BASE64', 'ANDROID_KEY_ALIAS', 'ANDROID_KEY_PASSWORD', 'ANDROID_STORE_PASSWORD']
    present = [bool(os.environ.get(name)) for name in names]
    if not any(present) and os.environ.get('REQUIRE_ANDROID_SIGNING') != 'true':
        export('ANDROID_SIGNED', 'false')
        print('Manual build: Android packages will be explicitly marked unsigned.')
        return
    required(names)
    properties = ROOT / 'apps/gui/src-tauri/gen/android/keystore.properties'
    if properties.exists():
        raise ValueError('Refusing to overwrite an existing Android signing configuration')
    key = Path(os.environ['RUNNER_TEMP']) / 'omnistream-signing/android.jks'
    private_file(key, decode(os.environ['ANDROID_KEYSTORE_BASE64']))
    secret_command('keytool', '-list', '-keystore', str(key), '-storepass', os.environ['ANDROID_STORE_PASSWORD'])
    values = {'storeFile': str(key), 'storePassword': os.environ['ANDROID_STORE_PASSWORD'],
              'keyAlias': os.environ['ANDROID_KEY_ALIAS'], 'keyPassword': os.environ['ANDROID_KEY_PASSWORD']}
    private_file(properties, ''.join(name + '=' + property_value(value) + '\n' for name, value in values.items()).encode())
    export('ANDROID_SIGNED', 'true')
    print('Android release signing configured.')


def profile_metadata(profile, identifier):
    team = profile['TeamIdentifier'][0]
    application = profile.get('Entitlements', {}).get('application-identifier', '')
    prefix, _, pattern = application.partition('.')
    if prefix not in profile.get('ApplicationIdentifierPrefix', [team]) or not fnmatch.fnmatchcase(identifier, pattern):
        raise ValueError('The iOS provisioning profile does not match the application identifier')
    configured_team = os.environ.get('APPLE_DEVELOPMENT_TEAM')
    if configured_team and configured_team != team:
        raise ValueError('APPLE_DEVELOPMENT_TEAM does not match the provisioning profile')
    return team, profile['UUID'], profile['Name']


def ios():
    if sys.platform != 'darwin':
        raise ValueError('iOS signing requires macOS')
    required(['IOS_CERTIFICATE_BASE64', 'IOS_CERTIFICATE_PASSWORD', 'IOS_PROVISIONING_PROFILE_BASE64'])
    directory = Path(os.environ['RUNNER_TEMP']) / 'omnistream-signing'
    certificate = directory / 'certificate.p12'
    encoded_profile = directory / 'profile.mobileprovision'
    private_file(certificate, decode(os.environ['IOS_CERTIFICATE_BASE64']))
    private_file(encoded_profile, decode(os.environ['IOS_PROVISIONING_PROFILE_BASE64']))
    profile = plistlib.loads(secret_command('security', 'cms', '-D', '-i', str(encoded_profile)))
    identifier = json.loads((ROOT / 'apps/gui/src-tauri/tauri.conf.json').read_text())['identifier']
    team, uuid, name = profile_metadata(profile, identifier)
    if not profile.get('ProvisionedDevices') or profile.get('Entitlements', {}).get('get-task-allow', False):
        raise ValueError('iOS release-testing requires an Ad Hoc distribution provisioning profile with registered devices')
    destination = Path.home() / 'Library/MobileDevice/Provisioning Profiles' / (uuid + '.mobileprovision')
    old_keychains = shlex.split(secret_command('security', 'list-keychains', '-d', 'user').decode())
    keychain = directory / 'release.keychain-db'
    state = {'keychain': str(keychain), 'profile': str(destination), 'keychains': old_keychains, 'restore_profile': destination.exists()}
    if destination.exists():
        shutil.copy2(destination, directory / 'previous.mobileprovision')
    private_file(directory / 'ios-state.json', json.dumps(state).encode())
    password = secrets.token_urlsafe(32)
    secret_command('security', 'create-keychain', '-p', password, str(keychain))
    secret_command('security', 'set-keychain-settings', '-lut', '21600', str(keychain))
    secret_command('security', 'unlock-keychain', '-p', password, str(keychain))
    secret_command('security', 'import', str(certificate), '-P', os.environ['IOS_CERTIFICATE_PASSWORD'], '-A', '-t', 'cert', '-f', 'pkcs12', '-k', str(keychain))
    secret_command('security', 'set-key-partition-list', '-S', 'apple-tool:,apple:', '-k', password, str(keychain))
    secret_command('security', 'list-keychains', '-d', 'user', '-s', str(keychain), *old_keychains)
    identities = secret_command('security', 'find-identity', '-v', '-p', 'codesigning', str(keychain)).decode()
    match = re.search(r'\b([A-F0-9]{40})\s+"', identities)
    if not match:
        raise ValueError('The imported iOS certificate has no valid code-signing identity')
    private_file(destination, encoded_profile.read_bytes())
    export('APPLE_DEVELOPMENT_TEAM', team)
    export('IOS_PROFILE_NAME', name)
    export('IOS_SIGNING_IDENTITY', match[1])
    print('iOS signing keychain and provisioning profile configured.')


def configure_ios_project():
    # Xcode's automatic signing expects an interactive Apple account; CI uses
    # the certificate and profile installed in the preceding step instead.
    ruby = '''require 'xcodeproj'
project_path = Dir['apps/gui/src-tauri/gen/apple/*.xcodeproj'].first or abort 'Missing generated Xcode project'
project = Xcodeproj::Project.open(project_path)
project.targets.select { |target| target.product_type == 'com.apple.product-type.application' }.each do |target|
  target.build_configurations.each do |configuration|
    configuration.build_settings['CODE_SIGN_STYLE'] = 'Manual'
    configuration.build_settings['DEVELOPMENT_TEAM'] = ENV.fetch('APPLE_DEVELOPMENT_TEAM')
    configuration.build_settings['PROVISIONING_PROFILE_SPECIFIER'] = ENV.fetch('IOS_PROFILE_NAME')
    configuration.build_settings['CODE_SIGN_IDENTITY'] = ENV.fetch('IOS_SIGNING_IDENTITY')
    configuration.build_settings['DEVELOPMENT_TEAM[sdk=iphoneos*]'] = ENV.fetch('APPLE_DEVELOPMENT_TEAM')
    configuration.build_settings['PROVISIONING_PROFILE_SPECIFIER[sdk=iphoneos*]'] = ENV.fetch('IOS_PROFILE_NAME')
    configuration.build_settings['CODE_SIGN_IDENTITY[sdk=iphoneos*]'] = ENV.fetch('IOS_SIGNING_IDENTITY')
  end
end
project.save
'''
    subprocess.run(['ruby', '-e', ruby], cwd=ROOT, check=True)
    # Tauri 2.12 only derives these export settings when its own certificate
    # environment variables are used. Our temporary keychain is already set up.
    identifier = json.loads((ROOT / 'apps/gui/src-tauri/tauri.conf.json').read_text())['identifier']
    options = {'method': 'release-testing', 'signingStyle': 'manual',
               'teamID': os.environ['APPLE_DEVELOPMENT_TEAM'],
               'signingCertificate': os.environ['IOS_SIGNING_IDENTITY'],
               'provisioningProfiles': {identifier: os.environ['IOS_PROFILE_NAME']}}
    (ROOT / 'apps/gui/src-tauri/gen/apple/ExportOptions.plist').write_bytes(plistlib.dumps(options))


def cleanup(platform):
    directory = Path(os.environ['RUNNER_TEMP']) / 'omnistream-signing'
    if platform == 'android':
        (ROOT / 'apps/gui/src-tauri/gen/android/keystore.properties').unlink(missing_ok=True)
        (directory / 'android.jks').unlink(missing_ok=True)
    else:
        state_file = directory / 'ios-state.json'
        if state_file.exists():
            state = json.loads(state_file.read_text())
            subprocess.run(['security', 'list-keychains', '-d', 'user', '-s', *state['keychains']], capture_output=True)
            subprocess.run(['security', 'delete-keychain', state['keychain']], capture_output=True)
            if state['restore_profile']:
                shutil.copy2(directory / 'previous.mobileprovision', state['profile'])
            else:
                Path(state['profile']).unlink(missing_ok=True)
        for filename in ['ios-state.json', 'certificate.p12', 'profile.mobileprovision', 'previous.mobileprovision']:
            (directory / filename).unlink(missing_ok=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['android', 'verify-android', 'ios', 'ios-project', 'cleanup-android', 'cleanup-ios'])
    args = parser.parse_args()
    if args.command.startswith('cleanup-'):
        cleanup(args.command.removeprefix('cleanup-'))
    elif args.command == 'android':
        android()
    elif args.command == 'verify-android':
        verify_android()
    elif args.command == 'ios':
        ios()
    else:
        configure_ios_project()
