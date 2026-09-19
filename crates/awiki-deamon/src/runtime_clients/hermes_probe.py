import importlib.machinery
import importlib.util
import sys

sys.path = [p for p in sys.path if p not in ("", ".")]
# Top-level discovery must honor sys.meta_path: official editable installs
# register a finder instead of placing the source directory on sys.path.
package = importlib.util.find_spec("tui_gateway")
locations = package.submodule_search_locations if package else None
# Searching the child via util.find_spec would import the parent package. Use
# its discovered locations directly so neither Hermes nor its entry is executed.
entry = (
    importlib.machinery.PathFinder.find_spec("tui_gateway.entry", locations)
    if locations
    else None
)
if not entry:
    sys.exit(2)

# Version is optional: source-only installs and damaged/missing metadata must
# not turn a discoverable Gateway into an unavailable client. Query the same
# interpreter's distribution metadata without importing any Hermes code.
try:
    import importlib.metadata
    import json

    installed_version = importlib.metadata.version("hermes-agent")
    if isinstance(installed_version, str):
        print(json.dumps({"awiki_hermes_version": installed_version}))
except Exception:
    pass
