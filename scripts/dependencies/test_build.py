import importlib.util
from pathlib import Path
import json
import tempfile
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('build_deps', Path(__file__).with_name('build.py'))
deps = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deps)
VERSIONS = {'anp': '1.0.1', 'anp-identity': '0.2.1', 'awiki-im-core': '0.1.1'}

class BuildDependencyTests(unittest.TestCase):
    def test_isolated_cli_carries_consumer_commit_and_rejects_wrong_metadata(self):
        with patch.dict(deps.os.environ, {'CARGO_TARGET_DIR': '/unrelated'}, clear=True):
            env = deps.consumer_environment('a' * 40, Path('/source-target'))
            self.assertEqual(env['AWIKI_CLI_COMMIT'], 'a' * 40)
            self.assertEqual(env['CARGO_TARGET_DIR'], '/source-target')
        with patch.dict(deps.os.environ, {'AWIKI_CLI_COMMIT': 'b' * 40}):
            with self.assertRaisesRegex(ValueError, 'metadata commit'):
                deps.consumer_environment('a' * 40, Path('/source-target'))

    def test_source_process_output_uses_utf8_for_non_ascii_git_paths(self):
        text = '路径/”source.rs'
        output = deps.run([sys.executable, '-c', "import sys; sys.stdout.buffer.write(bytes.fromhex('" + text.encode('utf-8').hex() + "'))"], capture=True)
        self.assertEqual(output, text)

    def test_source_cargo_keeps_exact_toolchain_features_and_locked_graph(self):
        command = deps.source_cargo_command(['+1.88.0', 'test', '-p', 'im-core-dart', '--no-default-features', '--features', 'windows,sqlite'])
        self.assertEqual(command[1:4], ['+1.88.0', 'test', '-p'])
        self.assertIn('--locked', command)
        self.assertEqual(command[-3:-1], ['--features', 'windows,sqlite'])
        self.assertEqual(deps.source_cargo_command(['build', '--locked']).count('--locked'), 1)

    def test_source_cargo_rejects_release_actions_and_workspace_escape(self):
        for command in [['publish'], ['install'], ['+stable', 'build'], ['build', '--manifest-path=/tmp/other.toml'], ['test', '--target-dir', '/tmp/cache'], ['build', '--config=x'], ['build', '--profile=release'], ['test', '--', '--ignored']]:
            with self.subTest(command=command), self.assertRaises(ValueError):
                deps.source_cargo_command(command)

    def test_source_cargo_cannot_enter_registry_or_release_modes(self):
        for args in [['--cargo-command', 'build'], ['--profile', 'release', '--deps', 'source', '--source-manifest', 'dependencies.source.json', '--cargo-command', 'build']]:
            with patch.object(deps, 'run') as run, self.assertRaises(SystemExit):
                deps.main(args)
            run.assert_not_called()
        with patch.dict(deps.os.environ, {'AWIKI_RELEASE_REGISTRY': '1'}), patch.object(deps, 'run') as run, self.assertRaises(SystemExit):
            deps.main(['--deps', 'source', '--source-manifest', 'dependencies.source.json', '--cargo-command', 'build'])
        run.assert_not_called()

    def test_fetched_sha_mismatch_is_rejected_before_checkout(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(deps, 'run', side_effect=[None, None, 'b' * 40 + '\n']) as run:
            with self.assertRaises(ValueError):
                deps.checkout_source({'repository': 'https://example.invalid/sdk.git', 'commit': 'a' * 40}, Path(temp) / 'sdk')
            self.assertFalse(any('checkout' in call.args[0] for call in run.call_args_list))

    def test_source_sha_and_repository_are_explicit(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'source.json'
            item = {'repository': 'https://github.com/example/sdk.git', 'commit': 'a' * 40, 'pull_request': 'https://github.com/example/sdk/pull/1'}
            def save(): path.write_text(json.dumps({'schema_version': 1, 'dependencies': {'anp': item}}))
            save(); self.assertEqual(deps.read_selection(path, 'source')['anp']['commit'], 'a' * 40)
            for field, value in [('commit', 'feature/foo'), ('repository', 'https://token@github.com/example/sdk')]:
                original = item[field]; item[field] = value; save()
                with self.assertRaises(ValueError): deps.read_selection(path, 'source')
                item[field] = original

    def test_local_config_cannot_be_used_as_pr_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'local.json'
            path.write_text(json.dumps({'schema_version': 1, 'dependencies': {'anp': {'path': '../my-anp'}}}))
            self.assertEqual(deps.read_selection(path, 'local')['anp']['path'], '../my-anp')
            with self.assertRaises(ValueError): deps.read_selection(path, 'source')

    def test_anp_only_example_is_valid_debug_local_config(self):
        example = Path(__file__).with_name('local-anp.example.json')
        selected = deps.read_selection(example, 'local')
        self.assertEqual(set(selected), {'anp'})
        self.assertEqual(selected['anp']['path'], '../anp/anp')

    def test_release_cannot_run_local_code(self):
        with patch.object(deps, 'run') as run:
            with self.assertRaises(SystemExit): deps.main(['--profile', 'release', '--deps', 'local'])
            run.assert_not_called()

    def test_metadata_checks_actual_selected_path_and_rejects_fallback(self):
        packages = [{'name': n, 'version': v, 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'manifest_path': '/registry/' + n + '/Cargo.toml'} for n, v in VERSIONS.items()]
        deps.verify_resolution({'packages': packages}, VERSIONS, {})
        roots = {'anp': Path('/selected')}
        with self.assertRaises(ValueError): deps.verify_resolution({'packages': packages}, VERSIONS, roots)
        packages[0].update(source=None, manifest_path='/selected/rust/Cargo.toml')
        deps.verify_resolution({'packages': packages}, VERSIONS, roots)
        with self.assertRaises(ValueError): deps.verify_resolution({'packages': packages}, VERSIONS, {})
        packages.append(dict(packages[0]))
        with self.assertRaises(ValueError): deps.verify_resolution({'packages': packages}, VERSIONS, roots)

    def test_development_snapshot_includes_dirty_and_new_source_not_ignored_config(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / 'source'; source.mkdir()
            deps.run(['git', 'init', '--quiet'], source)
            (source / '.gitignore').write_text('dependencies.local.json\n')
            (source / 'main.rs').write_text('original')
            deps.run(['git', 'add', '.'], source)
            deps.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture'], source)
            (source / 'main.rs').write_text('changed')
            (source / 'new.rs').write_text('new')
            (source / 'dependencies.local.json').write_text('private path')
            target = Path(temp) / 'copy'
            evidence = deps.copy_source(source, target)
            self.assertTrue(evidence['dirty'])
            self.assertEqual((target / 'main.rs').read_text(), 'changed')
            self.assertTrue((target / 'new.rs').exists())
            self.assertFalse((target / 'dependencies.local.json').exists())

if __name__ == '__main__': unittest.main()
