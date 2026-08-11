import importlib

# Module-level call-site string: c -> d whenever string imports are on.
TARGET = importlib.import_module("my.app.d")

# Registry-style string: only scanned in "all" mode (never forms a cycle;
# it is the only candidate naming my.app.b).
REGISTRY_ENTRY = "my.app.b"

# Self-string via call site: the self-edge is dropped, so this never forms
# a 1-file cycle.
SELF = importlib.import_module("my.app.c")

# f-string: the literal fragments are not candidates, but interpolations are.
LAZY = f"{importlib.import_module('my.app.e')}"


def load():
    # Nested call-site string: c -> e only when --local-imports is also on.
    return importlib.import_module("my.app.e")
