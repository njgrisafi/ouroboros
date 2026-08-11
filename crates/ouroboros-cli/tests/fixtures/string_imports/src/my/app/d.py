import importlib


def load():
    # Nested string candidate: d -> c only when --local-imports is also on.
    return importlib.import_module("my.app.c")
