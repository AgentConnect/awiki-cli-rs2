"""Read-only Windows sharing diagnostics. Never emits paths or file names."""
from collections import Counter
from pathlib import Path
import ctypes
from ctypes import wintypes
import json
import os
import sys

_DELETE=0x00010000
_SHARE_ALL=7
_OPEN_EXISTING=3
_BACKUP_SEMANTICS=0x02000000
_INVALID=ctypes.c_void_p(-1).value

def _kernel():
    k=ctypes.WinDLL('kernel32',use_last_error=True)
    k.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
    k.CreateFileW.restype=wintypes.HANDLE
    k.CloseHandle.argtypes=[wintypes.HANDLE]
    k.CloseHandle.restype=wintypes.BOOL
    return k

def _category(p):
    if p.is_dir():return 'directory'
    n=p.name.lower()
    if n in {'im_core.sqlite','local-state.sqlite'}:return 'core_sqlite'
    if n.startswith(('im_core.sqlite-','local-state.sqlite-')):return 'core_sqlite_journal'
    if n.startswith('awiki_me_product_store.db'):return 'product_sqlite'
    if n.endswith('.lock'):return 'lock_file'
    return 'other_file'

def probe(root,limit=512):
    if sys.platform!='win32':return {'status':'unsupported_platform'}
    root=Path(root)
    if not root.is_absolute() or not root.is_dir() or root.is_symlink() or root.is_junction():
        return {'status':'invalid_root'}
    k=_kernel();locked=Counter();errors=Counter();scanned=0;truncated=False
    def paths():
        yield root
        for base,dirs,files in os.walk(root,followlinks=False):
            dirs[:]=[d for d in dirs if not (Path(base)/d).is_symlink() and not (Path(base)/d).is_junction()]
            for name in dirs+files:
                p=Path(base)/name
                if not p.is_symlink() and not p.is_junction():yield p
    for p in paths():
        if scanned>=limit:truncated=True;break
        scanned+=1
        h=k.CreateFileW(str(p),_DELETE,_SHARE_ALL,None,_OPEN_EXISTING,_BACKUP_SEMANTICS,None)
        if h==_INVALID:
            error=ctypes.get_last_error()
            if error in (32,33):locked[_category(p)]+=1
            else:errors[str(error)]+=1
        else:k.CloseHandle(h)
    return {'status':'scanned','scannedCount':scanned,'lockedCategories':dict(locked),'systemErrorCounts':dict(errors),'truncated':truncated}

def main(argv):
    if len(argv)!=2 or argv[0]!='--root':return {'status':'invalid_request'}
    try:return probe(argv[1])
    except Exception:return {'status':'probe_failed'}

if __name__=='__main__':
    print(json.dumps(main(sys.argv[1:]),ensure_ascii=True))
