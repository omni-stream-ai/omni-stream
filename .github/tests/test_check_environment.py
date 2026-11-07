from contextlib import ExitStack
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import check_environment


class NativeProbeTests(unittest.TestCase):
    def test_apple_probes_verify_the_compiled_architecture_on_macos_and_ios(self):
        for target, ios, arch in [('aarch64-apple-darwin', False, 'arm64'),
                                  ('x86_64-apple-darwin', False, 'x86_64'),
                                  ('aarch64-apple-ios', True, 'arm64')]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                checked = []

                def native(*args):
                    if args[0] == 'xcrun':
                        return temporary
                    if args[0] == 'rustc':
                        return temporary
                    if args[0] == 'clang':
                        # Stand in for a thin Mach-O produced by clang.
                        Path(args[args.index('-o') + 1]).write_text(args[args.index('-arch') + 1])
                    if args[0] == 'lipo':
                        # lipo consumes every value following -verify_arch as an arch.
                        # Reproduce the real runner's failure for a trailing file path.
                        operation = args.index('-verify_arch')
                        requested = args[operation + 1:]
                        if len(args[1:operation]) != 1 or any(value not in {'arm64', 'x86_64'} for value in requested):
                            raise subprocess.CalledProcessError(1, args, 'unknown architecture specification flag')
                        self.assertEqual(Path(args[1]).read_text(), arch)
                        self.assertIn(arch, requested)
                        checked.append(arch)
                    return 'OK'

                with patch.object(check_environment, 'require_tools'), patch.object(check_environment, 'run', side_effect=native):
                    check_environment.apple(target, ios=ios)
                self.assertEqual(checked, [arch])

    def test_windows_requires_an_explicit_existing_linker_before_running_probes(self):
        with patch.object(check_environment, 'require_tools'), patch.object(check_environment, 'run') as run:
            with patch.dict(os.environ, {}, clear=True):
                with self.assertRaisesRegex(RuntimeError, 'native MSVC'):
                    check_environment.windows()
            with tempfile.TemporaryDirectory() as temporary:
                missing = str(Path(temporary) / 'missing-link.exe')
                with patch.dict(os.environ, {'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER': missing}, clear=True):
                    with self.assertRaisesRegex(RuntimeError, 'Missing build dependency'):
                        check_environment.windows()
            run.assert_not_called()

    def test_windows_c_probe_cannot_hide_a_cargo_host_linker_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            linker = Path(temporary) / 'link.exe'
            linker.touch()
            cargo_checked = []

            def native(*args):
                if args[0] == 'rustc':
                    return temporary
                if args[0] == 'cargo':
                    manifest = Path(args[args.index('--manifest-path') + 1])
                    self.assertEqual(tomllib.loads(manifest.read_text())['package']['name'], 'omnistream-linker-probe')
                    self.assertTrue((manifest.parent / 'build.rs').is_file())
                    self.assertTrue((manifest.parent / 'src/main.rs').is_file())
                    cargo_checked.append(True)
                    raise subprocess.CalledProcessError(101, args, 'host linker failed')
                return 'C and SDK probes passed'

            with ExitStack() as stack:
                stack.enter_context(patch.dict(os.environ, {'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER': str(linker)}, clear=True))
                stack.enter_context(patch.object(check_environment, 'require_tools'))
                stack.enter_context(patch.object(check_environment, 'run', side_effect=native))
                with self.assertRaisesRegex(subprocess.CalledProcessError, '101'):
                    check_environment.windows()
            self.assertEqual(cargo_checked, [True])


if __name__ == '__main__':
    unittest.main()
