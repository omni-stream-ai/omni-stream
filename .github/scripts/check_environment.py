"""Fail early when a native release toolchain cannot compile or find its SDK."""

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def run(*args):
    result = subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
    print(result.splitlines()[0] if result else 'OK: ' + str(args[0]), flush=True)
    return result


def require_path(path):
    if not path.exists():
        raise RuntimeError('Missing build dependency: ' + str(path))
    return path


def require_tools(*names):
    for name in names:
        if not shutil.which(name):
            raise RuntimeError('Missing build tool on PATH: ' + name)


def rust_target(target):
    require_path(Path(run('rustc', '--print', 'target-libdir', '--target', target)))


def linux():
    require_tools('cc', 'c++', 'make', 'perl', 'pkg-config', 'curl', 'wget', 'file', 'patchelf', 'python3')
    run('python3', '-c', 'import sys, tomllib; print("Core test fixture Python: " + sys.version.split()[0])')
    packages = ['dbus-1', 'gtk+-3.0', 'gdk-x11-3.0', 'webkit2gtk-4.1',
                'javascriptcoregtk-4.1', 'libsoup-3.0', 'librsvg-2.0', 'openssl',
                'ayatana-appindicator3-0.1']
    run('pkg-config', '--modversion', *packages)
    flags = shlex.split(run('pkg-config', '--cflags', '--libs', *packages))
    with tempfile.TemporaryDirectory(prefix='omnistream-linux-probe-') as temporary:
        source = Path(temporary) / 'probe.c'
        source.write_text('#include <dbus/dbus.h>\n#include <gtk/gtk.h>\n#include <openssl/crypto.h>\n'
                          'int main(void) { int a,b,c; dbus_get_version(&a,&b,&c); '
                          'return gtk_get_major_version() == 0 || OpenSSL_version_num() == 0; }\n')
        run('cc', str(source), '-o', str(source.with_suffix('')), *flags)
        run(str(source.with_suffix('')))
    rust_target('x86_64-unknown-linux-gnu')


def apple(target, ios=False):
    require_tools('clang', 'make', 'perl', 'xcrun', 'codesign', 'security', 'lipo')
    run('xcodebuild', '-version')
    sdk = run('xcrun', '--sdk', 'iphoneos' if ios else 'macosx', '--show-sdk-path')
    require_path(Path(sdk))
    arch = 'arm64' if target.startswith('aarch64-') else 'x86_64'
    with tempfile.TemporaryDirectory(prefix='omnistream-apple-probe-') as temporary:
        source = Path(temporary) / 'probe.c'
        source.write_text('int main(void) { return 0; }\n')
        options = ['-arch', arch, '-isysroot', sdk]
        if ios:
            minimum = json.loads((ROOT / 'apps/gui/src-tauri/tauri.ios.conf.json').read_text())['bundle']['iOS']['minimumSystemVersion']
            options += ['-target', 'arm64-apple-ios' + minimum]
        run('clang', *options, str(source), '-o', str(source.with_suffix('')))
        run('lipo', '-verify_arch', arch, str(source.with_suffix('')))
    rust_target(target)
    if ios:
        require_tools('ruby', 'pod', 'xcodegen')
        run('pod', '--version')
        run('xcodegen', '--version')
        run('ruby', '-rxcodeproj', '-e', 'puts Xcodeproj::VERSION')


def windows():
    require_tools('cl', 'link', 'nmake', 'perl', 'nasm')
    run(os.environ.get('OPENSSL_SRC_PERL', 'perl'), '-e',
        'die "OpenSSL requires native Windows Perl\n" unless $^O eq "MSWin32"; print "Native Windows Perl ready\n";')
    run('nasm', '-v')
    with tempfile.TemporaryDirectory(prefix='omnistream-windows-probe-') as temporary:
        source = Path(temporary) / 'probe.c'
        source.write_text('int main(void) { return 0; }\n')
        run('cl', '/nologo', str(source), '/Fo' + str(source.with_suffix('.obj')), '/Fe' + str(source.with_suffix('.exe')))
        run(str(source.with_suffix('.exe')))
    rust_target('x86_64-pc-windows-msvc')


def android():
    require_tools('java', 'javac', 'keytool', 'jarsigner', 'sdkmanager')
    run('java', '-version')
    run('javac', '-version')
    sdk = Path(os.environ['ANDROID_HOME'])
    ndk = Path(os.environ['NDK_HOME'])
    api = os.environ.get('ANDROID_API_LEVEL', '36')
    tools = os.environ.get('ANDROID_BUILD_TOOLS_VERSION', '36.0.0')
    expected_ndk = os.environ.get('ANDROID_NDK_VERSION', '28.2.13676358')
    require_path(sdk / ('platforms/android-' + api) / 'android.jar')
    for name in ['aapt2', 'apksigner']:
        run(str(require_path(sdk / 'build-tools' / tools / name)), 'version')
    require_path(sdk / 'build-tools' / tools / 'zipalign')
    properties = (require_path(ndk / 'source.properties')).read_text()
    if not any(line.strip() == 'Pkg.Revision = ' + expected_ndk for line in properties.splitlines()):
        raise RuntimeError('NDK_HOME does not match Android NDK ' + expected_ndk)
    # minSdkVersion is independent of the installed compile SDK platform.
    minimum = json.loads((ROOT / 'apps/gui/src-tauri/tauri.android.conf.json').read_text())['bundle']['android']['minSdkVersion']
    with tempfile.TemporaryDirectory(prefix='omnistream-android-probe-') as temporary:
        source = Path(temporary) / 'probe.c'
        source.write_text('int main(void) { return 0; }\n')
        for target, compiler in [('aarch64-linux-android', 'aarch64-linux-android'),
                                 ('armv7-linux-androideabi', 'armv7a-linux-androideabi'),
                                 ('x86_64-linux-android', 'x86_64-linux-android')]:
            tool = require_path(ndk / 'toolchains/llvm/prebuilt/linux-x86_64/bin' / (compiler + str(minimum) + '-clang'))
            run(str(tool), '--version')
            run(str(tool), str(source), '-o', str(source.parent / target))
            rust_target(target)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=['linux', 'macos', 'windows', 'android', 'ios'])
    parser.add_argument('--target', default='aarch64-apple-darwin')
    args = parser.parse_args()
    try:
        if args.platform in {'macos', 'ios'}:
            apple('aarch64-apple-ios' if args.platform == 'ios' else args.target, ios=args.platform == 'ios')
        else:
            {'linux': linux, 'windows': windows, 'android': android}[args.platform]()
    except subprocess.CalledProcessError as error:
        print(error.output or str(error), file=sys.stderr)
        sys.exit(1)
    except (OSError, RuntimeError, KeyError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    print('Native build environment verified: ' + args.platform)
