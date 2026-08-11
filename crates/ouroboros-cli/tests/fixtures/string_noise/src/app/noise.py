import importlib

# Dotted string that matches no first-party module, passed to a real
# dynamic-import call. With --include-string-imports it is a candidate,
# but must be dropped silently -- it is not an unresolved import and must
# not trip --strict.
X = importlib.import_module("no.such.module")
