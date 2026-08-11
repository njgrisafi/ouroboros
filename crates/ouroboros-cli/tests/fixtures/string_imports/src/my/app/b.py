import importlib

# Module-level call-site string: closes the a <-> b cycle when string
# imports are enabled (in both modes).
LOADED = importlib.import_module("my.app.a")

# Unresolvable candidate: silently dropped (not an unresolved import).
NOTE = "not.a.module"
