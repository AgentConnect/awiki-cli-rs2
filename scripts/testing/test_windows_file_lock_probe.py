from pathlib import Path
import ctypes
import importlib.util
import json
import tempfile
import sys
import unittest

spec=importlib.util.spec_from_file_location('owned_lock_probe',Path(__file__).with_name('windows_file_lock_probe.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

@unittest.skipUnless(sys.platform == "win32", "native Windows file sharing only")
class NativeProbeTest(unittest.TestCase):
    def test_real_sharing_violation_is_classified_without_leaking_paths_or_retaining_handles(self):
        with tempfile.TemporaryDirectory(prefix='awiki-lock-probe-') as name:
            root=Path(name);p=root/'im_core.sqlite';p.write_bytes(b'synthetic fixture only')
            k=module._kernel()
            # A genuine native handle which refuses FILE_SHARE_DELETE.
            held=k.CreateFileW(str(p),0x80000000,3,None,3,0,None)
            self.assertNotEqual(held,module._INVALID)
            try:
                result=module.probe(root)
                self.assertEqual(result['lockedCategories'],{'core_sqlite':1})
                self.assertEqual(result['systemErrorCounts'],{})
                encoded=json.dumps(result)
                self.assertNotIn(str(root),encoded)
                self.assertNotIn('im_core.sqlite',encoded)
            finally:k.CloseHandle(held)
            self.assertEqual(module.probe(root)['lockedCategories'],{})
            # The probe must close its own DELETE-access handles too.
            p.unlink()
    def test_invalid_input_does_not_echo_private_paths(self):
        self.assertEqual(module.main(['--unknown','synthetic-private-value']),{'status':'invalid_request'})
        self.assertEqual(module.main(['--root','synthetic-private-value']),{'status':'invalid_root'})
        with tempfile.TemporaryDirectory(prefix='awiki-lock-probe-') as name:
            result=module.probe(Path(name),limit=0)
            self.assertTrue(result['truncated'])
            self.assertEqual(result['scannedCount'],0)

if __name__=='__main__':unittest.main(verbosity=2)
