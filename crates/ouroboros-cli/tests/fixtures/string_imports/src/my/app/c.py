import importlib

# Module-level string candidate: c -> d whenever string imports are on.
TARGET = "my.app.d"

# Self-string: the self-edge is dropped, so this never forms a 1-file cycle.
SELF = "my.app.c"

# f-string: the literal fragments are not candidates, but interpolations are.
LAZY = f"{importlib.import_module('my.app.e')}"


def load():
    # Nested string candidate: c -> e only when --local-imports is also on.
    return importlib.import_module("my.app.e")
