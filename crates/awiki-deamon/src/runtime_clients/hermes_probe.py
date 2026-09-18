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
sys.exit(0 if entry else 2)
