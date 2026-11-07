import base64
from contextlib import ExitStack
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import artifacts
import release
import signing


class Workspace(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = self.root / 'release-assets'
        self.assets.mkdir()
        self.write_version('0.1.0')

    def write(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
        return path

    def write_version(self, value):
        self.write(release.CONFIG, json.dumps({'version': value, 'identifier': 'com.omnistreamai.app'}))
        self.write('Cargo.toml', '[workspace.package]\nversion = ' + json.dumps(value))
        self.write('apps/gui/src-tauri/Cargo.toml', '[package]\nversion = ' + json.dumps(value))
        self.write('apps/gui/package.json', json.dumps({'version': value}))

    def git(self, *args):
        return release.command('git', *args, root=self.root)

    def initialize_git(self):
        self.git('init', '-b', 'main')
        self.git('config', 'user.name', 'Release Test')
        self.git('config', 'user.email', 'release@example.invalid')
        return self.commit('chore: initial')

    def commit(self, subject):
        self.git('add', '.')
        self.git('commit', '--allow-empty', '-m', subject)
        return self.git('rev-parse', 'HEAD')

    def plan(self, before, branch='main', event='push'):
        return release.plan(event, {'before': before, 'repository': {'default_branch': 'main'}},
                            branch, self.git('rev-parse', 'HEAD'), root=self.root)


class VersionTests(Workspace):
    def test_version_consistency_and_semver(self):
        self.assertEqual(release.version(self.root), '0.1.0')
        self.write('apps/gui/package.json', '{"version":"0.2.0"}')
        with self.assertRaisesRegex(ValueError, 'must match'):
            release.version(self.root)
        for value in ['01.2.0', '1.2', '1.0.0+build', '1.0.0-beta.01']:
            self.write_version(value)
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.version(self.root)

    def test_manual_is_build_only(self):
        sha = self.initialize_git()
        self.write_version('0.1.0-beta.1')
        with patch.dict(os.environ, {'ENABLE_IOS_RELEASE': 'true'}):
            result = self.plan(sha, event='workflow_dispatch')
        self.assertEqual((result['should_build'], result['should_release'], result['ios_enabled']), ('true', 'false', 'true'))

    def test_branch_and_version_policy(self):
        initial = self.initialize_git()
        self.write_version('0.2.0')
        sha = self.commit('feat: new release')
        self.assertEqual(self.plan(initial)['should_release'], 'true')
        self.assertEqual(self.plan(initial, branch='feature')['should_release'], 'false')
        self.git('tag', 'v0.2.0', initial)
        with self.assertRaisesRegex(ValueError, 'another commit'):
            self.plan(initial)
        self.git('tag', '-d', 'v0.2.0')
        self.write_version('0.3.0-beta.1')
        self.commit('feat: preview')
        self.assertEqual(self.plan(sha, branch='preview')['should_release'], 'true')
        with self.assertRaisesRegex(ValueError, 'default branch'):
            self.plan(sha)

    def test_pipeline_first_release_and_draft_retry(self):
        initial = self.initialize_git()
        self.write('.github/workflows/release.yml', 'name: Release')
        sha = self.commit('ci: add release')
        self.assertEqual(self.plan(initial)['should_release'], 'true')
        self.git('tag', 'v0.1.0', sha)
        self.assertEqual(self.plan(initial)['should_release'], 'true')
        self.write('.github/scripts/build.py', '# improved packaging')
        self.commit('ci: improve packaging')
        result = self.plan(sha)
        self.assertEqual((result['should_build'], result['should_release']), ('true', 'false'))

    def test_manifest_dependency_edit_does_not_publish(self):
        initial = self.initialize_git()
        with (self.root / 'Cargo.toml').open('a') as output:
            output.write('\n# dependency change')
        self.commit('chore: dependencies')
        self.assertEqual(self.plan(initial)['should_build'], 'false')
        self.assertEqual(self.plan('0' * 40, branch='new-branch')['should_release'], 'false')

    def test_changelog_reachable_tags_categories_and_range(self):
        initial = self.initialize_git()
        self.git('tag', 'v0.1.0')
        self.git('checkout', '-b', 'unrelated')
        self.commit('feat: unrelated future')
        self.git('tag', 'v99.0.0')
        self.git('checkout', 'main')
        self.commit('feat: native apps')
        self.git('tag', 'v0.2.0-beta.1')
        self.commit('fix(gui): reconnect')
        self.commit('feat(api)!: new protocol')
        sha = self.commit('perf: less work')
        previous = release.previous_tag('v0.2.0', sha, self.root)
        self.assertEqual(previous, 'v0.1.0')
        self.assertEqual(release.previous_tag('v0.2.0-beta.2', sha, self.root), 'v0.2.0-beta.1')
        body = release.notes('v0.2.0', previous, sha, 'owner/repo', False, self.root)
        for title in ['Features', 'Fixes', 'Breaking changes', 'Performance']:
            self.assertIn('## ' + title, body)
        self.assertNotIn('chore: initial', body)
        self.assertNotIn('unrelated future', body)
        self.assertIn('/compare/v0.1.0...v0.2.0', body)
        self.assertIn('/commit/' + initial[:7], release.notes('v0.1.0', '', initial, 'owner/repo', True, self.root))

    def test_tag_creation_retry_and_collision_with_local_remote(self):
        sha = self.initialize_git()
        remote = self.root.parent / (self.root.name + '-remote.git')
        self.addCleanup(lambda: shutil.rmtree(remote, ignore_errors=True))
        subprocess.run(['git', 'init', '--bare', str(remote)], check=True, capture_output=True)
        self.git('remote', 'add', 'origin', str(remote))
        release.ensure_tag('v0.1.0', sha, self.root)
        release.ensure_tag('v0.1.0', sha, self.root)
        self.assertEqual(self.git('rev-parse', 'v0.1.0^{commit}'), sha)
        newer = self.commit('fix: another commit')
        with self.assertRaisesRegex(ValueError, 'another commit'):
            release.ensure_tag('v0.1.0', newer, self.root)


class ArtifactTests(Workspace):
    def fixtures(self, include_ios=False, signed=True):
        manifests = {}
        for group in artifacts.GROUPS | ({'ios'} if include_ios else set()):
            files = []
            for name in sorted(artifacts.expected_names(group, signed if group == 'android' else True)):
                path = self.write('release-assets/' + name, 'package ' + name)
                files.append({'name': name, 'sha256': artifacts.digest(path), 'size': path.stat().st_size})
            data = {'group': group, 'version': '0.1.0', 'commit': 'test-sha', 'files': files}
            if group in {'android', 'ios'}:
                data['signed'] = signed
            self.write('release-assets/' + group + '.manifest.json', json.dumps(data))
            manifests[group] = data
        return manifests

    def test_complete_assets_and_global_checksums(self):
        self.fixtures(include_ios=True)
        with patch.object(artifacts, 'DEST', self.assets):
            paths = artifacts.collect('0.1.0', 'test-sha', include_ios=True)
        self.assertEqual(len(paths), 18)
        self.assertEqual(len((self.assets / 'SHA256SUMS').read_text().splitlines()), 17)
        self.assertTrue(all(not path.name.endswith('.manifest.json') for path in paths))
        self.assertEqual(json.loads((self.assets / 'manifest.json').read_text())['commit'], 'test-sha')

    def test_tampering_missing_packages_and_wrong_commit_fail(self):
        manifests = self.fixtures()
        for commit in ['different-sha']:
            with self.assertRaisesRegex(ValueError, 'mismatch'):
                artifacts.verify_all(self.assets, '0.1.0', commit)
        package = self.assets / 'omnistream-web.tar.gz'
        package.write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'integrity'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')
        self.fixtures()
        manifests['android']['files'].pop()
        self.write('release-assets/android.manifest.json', json.dumps(manifests['android']))
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')
        self.fixtures()
        (self.assets / 'android.manifest.json').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing artifact'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')

    def test_unsigned_manual_artifacts_cannot_be_published(self):
        self.fixtures(signed=False)
        artifacts.verify_all(self.assets, '0.1.0', 'test-sha', require_signed=False)
        with self.assertRaisesRegex(ValueError, 'signed android'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')

    def test_unexpected_names_duplicates_and_symlinks(self):
        manifests = self.fixtures()
        android = manifests['android']
        android['files'][0]['name'] = '../outside'
        self.write('release-assets/android.manifest.json', json.dumps(android))
        with self.assertRaises(ValueError):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')
        manifests = self.fixtures()
        manifests['android']['files'].append(manifests['android']['files'][0])
        self.write('release-assets/android.manifest.json', json.dumps(manifests['android']))
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')
        self.fixtures()
        package = self.assets / 'omnistream-web.tar.gz'
        copy = self.write('outside.tar.gz', package.read_text())
        package.unlink()
        package.symlink_to(copy)
        with self.assertRaisesRegex(ValueError, 'integrity'):
            artifacts.verify_all(self.assets, '0.1.0', 'test-sha')

    def test_core_sidecar_archive_permissions_and_clean_staging(self):
        target = 'x86_64-unknown-linux-gnu'
        core = self.write('target/' + target + '/release/omnistream', 'core executable')
        core.chmod(0o755)
        self.write('target/release-core/linux-x86_64/stale', 'stale file')
        self.write('apps/gui/src-tauri/target/' + target + '/release/bundle/appimage/OmniStream.AppImage', 'appimage')
        self.write('apps/gui/src-tauri/target/' + target + '/release/bundle/deb/OmniStream.deb', 'deb')
        with patch.object(artifacts, 'ROOT', self.root), patch.object(artifacts, 'DEST', self.assets), patch.dict(os.environ, {'GITHUB_SHA': 'test-sha'}):
            artifacts.sidecar(target)
            artifacts.desktop('linux-x86_64')
        self.assertEqual((self.root / ('apps/gui/src-tauri/binaries/omnistream-' + target)).read_text(), 'core executable')
        with tarfile.open(self.assets / 'omnistream-core-linux-x86_64.tar.gz') as archive:
            self.assertEqual(archive.getnames(), ['omnistream'])
            self.assertEqual(archive.getmember('omnistream').mode & 0o111, 0o111)

    def test_web_archive_root_and_android_output_layout(self):
        self.write('apps/gui/dist/index.html', '<html></html>')
        self.write('apps/gui/dist/assets/main.js', 'app')
        outputs = 'apps/gui/src-tauri/gen/android/app/build/outputs/'
        for flavor in ['universal', 'arm', 'arm64', 'x86_64']:
            self.write(outputs + 'apk/' + flavor + '/release/app.apk', 'apk')
        self.write(outputs + 'bundle/universalRelease/app.aab', 'aab')
        with patch.object(artifacts, 'ROOT', self.root), patch.object(artifacts, 'DEST', self.assets), patch.dict(os.environ, {'GITHUB_SHA': 'test-sha', 'ANDROID_SIGNED': 'true'}):
            artifacts.archive(self.root / 'apps/gui/dist', 'omnistream-web.tar.gz')
            artifacts.android()
        with tarfile.open(self.assets / 'omnistream-web.tar.gz') as archive:
            self.assertEqual(set(archive.getnames()), {'index.html', 'assets/main.js'})
        self.assertTrue((self.assets / 'omnistream-android-arm64-v8a.apk').is_file())
        self.assertTrue(json.loads((self.assets / 'android.manifest.json').read_text())['signed'])


class SigningTests(Workspace):
    def test_manual_missing_partial_and_required_android_secrets(self):
        environment = {'GITHUB_ENV': str(self.root / 'env'), 'RUNNER_TEMP': str(self.root)}
        with patch.dict(os.environ, environment, clear=True):
            signing.android()
            self.assertIn('ANDROID_SIGNED=false', (self.root / 'env').read_text())
            os.environ['ANDROID_KEY_ALIAS'] = 'alias'
            with self.assertRaisesRegex(ValueError, 'ANDROID_KEYSTORE_BASE64'):
                signing.android()
            del os.environ['ANDROID_KEY_ALIAS']
            os.environ['REQUIRE_ANDROID_SIGNING'] = 'true'
            with self.assertRaisesRegex(ValueError, 'ANDROID_KEYSTORE_BASE64'):
                signing.android()

    def test_android_properties_and_cleanup(self):
        environment = {'GITHUB_ENV': str(self.root / 'env'), 'RUNNER_TEMP': str(self.root),
                       'ANDROID_KEYSTORE_BASE64': base64.b64encode(b'keystore').decode(),
                       'ANDROID_KEY_ALIAS': 'release', 'ANDROID_KEY_PASSWORD': 'a:b=c',
                       'ANDROID_STORE_PASSWORD': ' password\n'}
        with patch.dict(os.environ, environment, clear=True), patch.object(signing, 'ROOT', self.root), patch.object(signing, 'secret_command'):
            signing.android()
            key = self.root / 'omnistream-signing/android.jks'
            self.assertEqual(key.read_bytes(), b'keystore')
            self.assertEqual(key.stat().st_mode & 0o777, 0o600)
            properties = self.root / 'apps/gui/src-tauri/gen/android/keystore.properties'
            self.assertIn('keyPassword=a\\:b\\=c', properties.read_text())
            with self.assertRaisesRegex(ValueError, 'overwrite'):
                signing.android()
            signing.cleanup('android')
            self.assertFalse(properties.exists())
            self.assertFalse(key.exists())
        self.assertEqual(signing.property_value('密😀'), r'\u5bc6\ud83d\ude00')
        with self.assertRaises(ValueError):
            signing.decode('not!base64')

    def test_secret_command_failure_is_sanitized(self):
        failed = subprocess.CompletedProcess([], 1, b'private-output', b'secret-password')
        with patch.object(signing.subprocess, 'run', return_value=failed):
            with self.assertRaises(ValueError) as raised:
                signing.secret_command('keytool', '-storepass', 'secret-password')
        self.assertNotIn('secret-password', str(raised.exception))
        self.assertNotIn('private-output', str(raised.exception))

    def test_ios_profile_identifier_and_team(self):
        profile = {'TeamIdentifier': ['TEAM'], 'UUID': 'profile-id', 'Name': 'Ad Hoc',
                   'Entitlements': {'application-identifier': 'TEAM.com.omnistreamai.*'}}
        with patch.dict(os.environ, {'APPLE_DEVELOPMENT_TEAM': 'TEAM'}):
            self.assertEqual(signing.profile_metadata(profile, 'com.omnistreamai.app'), ('TEAM', 'profile-id', 'Ad Hoc'))
            with self.assertRaisesRegex(ValueError, 'identifier'):
                signing.profile_metadata(profile, 'com.example.app')
            os.environ['APPLE_DEVELOPMENT_TEAM'] = 'OTHER'
            with self.assertRaisesRegex(ValueError, 'TEAM'):
                signing.profile_metadata(profile, 'com.omnistreamai.app')

    def test_ios_export_options_use_installed_identity_and_profile(self):
        project = self.root / 'apps/gui/src-tauri/gen/apple'
        project.mkdir(parents=True)
        environment = {'APPLE_DEVELOPMENT_TEAM': 'TEAM', 'IOS_PROFILE_NAME': 'Ad Hoc Profile', 'IOS_SIGNING_IDENTITY': 'ABC123'}
        with patch.object(signing, 'ROOT', self.root), patch.dict(os.environ, environment), patch.object(signing.subprocess, 'run'):
            signing.configure_ios_project()
        options = plistlib.loads((project / 'ExportOptions.plist').read_bytes())
        self.assertEqual(options['signingCertificate'], 'ABC123')
        self.assertEqual(options['provisioningProfiles'], {'com.omnistreamai.app': 'Ad Hoc Profile'})
        self.assertEqual(options['method'], 'release-testing')


class PublicationTests(Workspace):
    def publish(self, existing=None, fail_upload=False):
        calls = []
        self.calls = calls
        asset = self.write('release-assets/package.zip', 'package')

        def run(args, **kwargs):
            calls.append((args, kwargs))
            if fail_upload and args[:3] == ['gh', 'release', 'upload']:
                raise subprocess.CalledProcessError(1, args)
            if args[:2] == ['gh', 'api'] and args[-1].endswith('/tags/v0.1.0'):
                return subprocess.CompletedProcess(args, 0 if existing else 1, json.dumps(existing), '' if existing else 'HTTP 404')
            return subprocess.CompletedProcess(args, 0, '', '')

        with ExitStack() as stack:
            stack.enter_context(patch.dict(os.environ, {'GITHUB_SHA': 'sha', 'GITHUB_REPOSITORY': 'owner/repo', 'ENABLE_IOS_RELEASE': 'false'}))
            stack.enter_context(patch.object(release, 'ROOT', self.root))
            stack.enter_context(patch.object(release, 'version', return_value='0.1.0'))
            stack.enter_context(patch.object(release.artifacts, 'collect', return_value=[asset]))
            stack.enter_context(patch.object(release, 'ensure_tag'))
            stack.enter_context(patch.object(release, 'previous_tag', return_value=''))
            stack.enter_context(patch.object(release, 'notes', return_value='Release notes'))
            stack.enter_context(patch.object(release, 'command', return_value='{"id":123,"draft":true}'))
            stack.enter_context(patch.object(release.subprocess, 'run', side_effect=run))
            release.publish()
        return calls

    def test_new_release_stays_draft_until_all_assets_upload(self):
        calls = self.publish()
        commands = [args for args, _ in calls]
        self.assertIn('--draft', commands[1])
        self.assertEqual(commands[2][:3], ['gh', 'release', 'upload'])
        self.assertEqual(commands[3][:4], ['gh', 'api', '--method', 'PATCH'])
        self.assertFalse(json.loads(calls[3][1]['input'])['draft'])

    def test_existing_draft_repaired_without_recreating(self):
        calls = self.publish({'id': 123, 'draft': True})
        self.assertEqual(len(calls), 3)
        self.assertIn('--clobber', calls[1][0])

    def test_failed_upload_does_not_promote_the_draft(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.publish(fail_upload=True)
        self.assertFalse(any('--method' in args for args, _ in self.calls))

    def test_published_release_is_immutable_and_requires_complete_assets(self):
        calls = self.publish({'id': 123, 'draft': False, 'assets': [{'name': 'package.zip'}]})
        self.assertEqual(len(calls), 1)
        with self.assertRaisesRegex(ValueError, 'missing assets'):
            self.publish({'id': 123, 'draft': False, 'assets': []})


if __name__ == '__main__':
    unittest.main()
