#!/usr/bin/env python3
"""Generate a sample Python project for testing ouroboros.

Usage:
    python fixtures/generate.py              # default ~30 files (scale 1)
    python fixtures/generate.py --scale 10   # ~250 files
    python fixtures/generate.py --scale 100  # ~2500 files
"""

from __future__ import annotations

import argparse
import os
import random
import shutil
import textwrap
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
OUTPUT_DIR = SCRIPT_DIR / "sample_project"

# ---------------------------------------------------------------------------
# Stdlib modules used when generating files
# ---------------------------------------------------------------------------
STDLIB_MODULES = [
    "os",
    "sys",
    "json",
    "csv",
    "io",
    "re",
    "math",
    "random",
    "hashlib",
    "secrets",
    "logging",
    "pathlib",
    "datetime",
    "platform",
    "functools",
    "collections",
    "typing",
    "dataclasses",
    "itertools",
    "contextlib",
    "abc",
    "enum",
    "copy",
    "uuid",
    "textwrap",
    "string",
    "operator",
]

# ---------------------------------------------------------------------------
# Base skeleton — hand-crafted ~30 files
# ---------------------------------------------------------------------------

BASE_FILES: dict[str, str] = {
    # -- root --
    "app.py": textwrap.dedent("""\
        from core.engine import Engine
        from models import ModelRegistry
        from models.user import User
        from services.auth.login import login
        from api.endpoints.users import get_users


        def main():
            engine = Engine()
            user = User("admin")
            ModelRegistry.register(type(user))
            login(user)
            get_users()
    """),
    "config.py": textwrap.dedent("""\
        import os
        import sys
        import json
        from pathlib import Path


        BASE_DIR = Path(__file__).resolve().parent
        CONFIG_PATH = os.environ.get("CONFIG_PATH", str(BASE_DIR / "config.json"))


        def load_config():
            if os.path.exists(CONFIG_PATH):
                with open(CONFIG_PATH) as f:
                    return json.load(f)
            return {}
    """),
    # -- core --
    "core/__init__.py": "",
    "core/engine.py": textwrap.dedent("""\
        from models.base import BaseModel
        from utils.helpers import slugify


        class Engine:
            def __init__(self):
                self.model = BaseModel()

            def run(self):
                return slugify(self.model.name)
    """),
    "core/runner.py": textwrap.dedent("""\
        from . import engine


        class Runner:
            def __init__(self):
                self.engine = engine.Engine()

            def execute(self):
                return self.engine.run()
    """),
    # -- models --
    "models/__init__.py": textwrap.dedent("""\
        class ModelRegistry:
            _models = {}

            @classmethod
            def register(cls, model_cls):
                cls._models[model_cls.__name__] = model_cls
                return model_cls

            @classmethod
            def get(cls, name):
                return cls._models.get(name)
    """),
    "models/base.py": textwrap.dedent("""\
        class BaseModel:
            name = "base"

            def get_engine(self):
                from core.engine import Engine  # local import to break circular dep
                return Engine()
    """),
    "models/user.py": textwrap.dedent("""\
        from .base import BaseModel


        class User(BaseModel):
            def __init__(self, username: str):
                self.username = username
                self.name = username
    """),
    "models/permissions.py": textwrap.dedent("""\
        from typing import List
        from dataclasses import dataclass, field
        from .user import User


        @dataclass
        class Permission:
            name: str
            users: List[User] = field(default_factory=list)
    """),
    # -- utils --
    "utils/__init__.py": "",
    "utils/helpers.py": textwrap.dedent("""\
        import os
        from collections import OrderedDict


        def slugify(text: str) -> str:
            return text.lower().replace(" ", "-")


        def env_dict() -> OrderedDict:
            return OrderedDict(sorted(os.environ.items()))


        def make_title(text: str) -> str:
            from .formatting import format_title  # local import to break circular dep
            return format_title(text)
    """),
    "utils/formatting.py": textwrap.dedent("""\
        from .helpers import slugify


        def format_title(text: str) -> str:
            return text.title()


        def format_slug(text: str) -> str:
            return slugify(text)
    """),
    "utils/validation.py": textwrap.dedent("""\
        from models.user import User


        def validate_username(user: User) -> bool:
            return len(user.username) >= 3
    """),
    # -- services/auth --
    "services/__init__.py": "",
    "services/auth/__init__.py": "",
    "services/auth/login.py": textwrap.dedent("""\
        from .session import create_session
        from models.user import User


        def login(user: User):
            session = create_session(user)
            return session
    """),
    "services/auth/session.py": textwrap.dedent("""\
        from .login import login


        def create_session(user):
            return {"user": user.username, "active": True}


        def refresh_session(user):
            login(user)
    """),
    "services/auth/tokens.py": textwrap.dedent("""\
        import secrets


        def generate_token(length: int = 32) -> str:
            import hashlib  # local import
            raw = secrets.token_bytes(length)
            return hashlib.sha256(raw).hexdigest()


        def validate_token(token: str) -> bool:
            from ..notifications.email import send_email  # relative double-dot + local
            send_email("admin@example.com", "token_validated")
            return len(token) == 64
    """),
    # -- services/notifications --
    "services/notifications/__init__.py": "",
    "services/notifications/email.py": textwrap.dedent("""\
        from .templates import render_template


        def send_email(to: str, template_name: str):
            body = render_template(template_name, recipient=to)
            return {"to": to, "body": body}
    """),
    "services/notifications/sms.py": textwrap.dedent("""\
        from .templates import render_template


        def send_sms(to: str, template_name: str):
            body = render_template(template_name, recipient=to)
            return {"to": to, "body": body}
    """),
    "services/notifications/templates.py": textwrap.dedent("""\
        from utils.formatting import format_title


        def render_template(name: str, **kwargs) -> str:
            title = format_title(name)
            parts = [f"{k}={v}" for k, v in kwargs.items()]
            return f"{title}: {', '.join(parts)}"
    """),
    # -- data/loaders --
    "data/__init__.py": "",
    "data/loaders/__init__.py": "",
    "data/loaders/csv_loader.py": textwrap.dedent("""\
        import csv
        import io


        def load_csv(raw: str):
            reader = csv.reader(io.StringIO(raw))
            return list(reader)
    """),
    "data/loaders/json_loader.py": textwrap.dedent("""\
        import json
        from pathlib import Path


        def load_json(filepath: str):
            path = Path(filepath)
            with path.open() as f:
                return json.load(f)
    """),
    "data/loaders/base_loader.py": textwrap.dedent("""\
        from .csv_loader import load_csv


        class BaseLoader:
            def load(self, raw: str):
                return load_csv(raw)
    """),
    # -- data/processors --
    "data/processors/__init__.py": "",
    "data/processors/pipeline.py": textwrap.dedent("""\
        from ..loaders.base_loader import BaseLoader
        from .transform import apply_transform


        class Pipeline:
            def __init__(self):
                self.loader = BaseLoader()

            def run(self, raw: str):
                data = self.loader.load(raw)
                return apply_transform(data)
    """),
    "data/processors/transform.py": textwrap.dedent("""\
        from .filters import regex_filter


        def apply_transform(data):
            return [row for row in data if row]


        def filter_and_transform(data, pattern):
            filtered = regex_filter(pattern, data)
            return apply_transform(filtered)


        def build_pipeline():
            from .pipeline import Pipeline  # local import to break circular dep
            return Pipeline()
    """),
    "data/processors/filters.py": textwrap.dedent("""\
        import re
        import functools
        from .pipeline import Pipeline


        def regex_filter(pattern: str, items: list[str]) -> list[str]:
            compiled = re.compile(pattern)
            return list(filter(compiled.match, items))


        def chain_filters(*funcs):
            return functools.reduce(lambda f, g: lambda x: g(f(x)), funcs)


        def filtered_pipeline(pattern: str, raw: str):
            p = Pipeline()
            data = p.run(raw)
            return regex_filter(pattern, [str(row) for row in data])
    """),
    # -- api/endpoints --
    "api/__init__.py": "",
    "api/endpoints/__init__.py": "",
    "api/endpoints/users.py": textwrap.dedent("""\
        from models.user import User
        from services.auth.login import login


        def get_users():
            return [User("alice"), User("bob")]


        def authenticate_user(username: str):
            user = User(username)
            return login(user)
    """),
    "api/endpoints/health.py": textwrap.dedent("""\
        import datetime
        import platform


        def health_check():
            return {
                "status": "ok",
                "time": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "platform": platform.system(),
            }
    """),
    # -- api/middleware --
    "api/middleware/__init__.py": "",
    "api/middleware/logging_mw.py": textwrap.dedent("""\
        import logging
        from core.engine import Engine

        logger = logging.getLogger(__name__)


        def log_request(request):
            logger.info("Processing request: %s", request)
            engine = Engine()
            return engine
    """),
    "api/middleware/rate_limit.py": textwrap.dedent("""\
        from .logging_mw import log_request


        class RateLimiter:
            def __init__(self, max_requests: int = 100):
                self.max_requests = max_requests
                self.count = 0

            def check(self, request):
                self.count += 1
                if self.count > self.max_requests:
                    raise RuntimeError("Rate limit exceeded")
                log_request(request)
    """),
}


# ---------------------------------------------------------------------------
# Procedural generation for --scale > 1
# ---------------------------------------------------------------------------

def _module_name_from_path(rel_path: str) -> str:
    """Convert a relative file path to a dotted module name.

    e.g. 'data/loaders/csv_loader.py' -> 'data.loaders.csv_loader'
         'core/__init__.py' -> 'core'
    """
    stem = rel_path.removesuffix(".py")
    parts = stem.replace("/", ".").replace("\\", ".")
    if parts.endswith(".__init__"):
        parts = parts[: -len(".__init__")]
    return parts


def _generate_scaled_files(
    scale: int,
    rng: random.Random,
) -> dict[str, str]:
    """Procedurally generate extra files for scale > 1.

    Returns a dict of relative-path -> source-code, just like BASE_FILES.
    """
    if scale <= 1:
        return {}

    # Collect existing module names from the base skeleton (non-init files).
    base_modules = [
        _module_name_from_path(p)
        for p in BASE_FILES
        if not p.endswith("__init__.py") and p.endswith(".py")
    ]

    generated_files: dict[str, str] = {}
    generated_modules: list[str] = []

    # We will create (scale - 1) "groups", each adding ~25 files.
    for group_idx in range(1, scale):
        group_files: list[tuple[str, str]] = []  # (rel_path, module_name)

        # Pick a random nesting depth pattern for this group.
        # Depths: 1 = flat (gen_XX/), 2-5 = nested.
        num_files_in_group = rng.randint(20, 30)

        # Create a few package prefixes for this group.
        num_packages = rng.randint(2, 5)
        packages: list[str] = []
        for pkg_idx in range(num_packages):
            depth = rng.randint(1, 5)
            parts = [f"gen{group_idx}"]
            for d in range(depth - 1):
                parts.append(f"sub{pkg_idx}_{d}")
            packages.append("/".join(parts))

        # Ensure __init__.py files for every package prefix.
        for pkg_path in packages:
            segments = pkg_path.split("/")
            for i in range(1, len(segments) + 1):
                init_path = "/".join(segments[:i]) + "/__init__.py"
                if init_path not in generated_files and init_path not in BASE_FILES:
                    generated_files[init_path] = ""

        # Distribute files across packages.
        for file_idx in range(num_files_in_group):
            pkg_path = rng.choice(packages)
            module_short = f"mod_{file_idx}"
            rel_path = f"{pkg_path}/{module_short}.py"
            module_name = _module_name_from_path(rel_path)
            group_files.append((rel_path, module_name))
            generated_modules.append(module_name)

        # All modules available for importing.
        available_modules = base_modules + generated_modules

        # Build a mapping from package path to list of (idx, module_short) for
        # sibling-based relative imports.
        pkg_to_files: dict[str, list[tuple[int, str]]] = {}
        for idx, (rel_path, _module_name) in enumerate(group_files):
            pkg_path = "/".join(rel_path.split("/")[:-1])
            short_name = rel_path.split("/")[-1].removesuffix(".py")
            pkg_to_files.setdefault(pkg_path, []).append((idx, short_name))

        # Decide which files get each import style.
        # Proportions: circular ~15%, stdlib ~20%, relative ~15%,
        #              local-in-func ~10%, normal ~40%.
        file_indices = list(range(len(group_files)))
        rng.shuffle(file_indices)

        n = len(group_files)
        num_circular = max(2, int(n * 0.15))
        num_stdlib = max(1, int(n * 0.20))
        num_relative = max(1, int(n * 0.15))
        num_local = max(1, int(n * 0.10))

        cursor = 0
        circular_indices = set(file_indices[cursor : cursor + num_circular])
        cursor += num_circular
        stdlib_indices = set(file_indices[cursor : cursor + num_stdlib])
        cursor += num_stdlib
        relative_indices = set(file_indices[cursor : cursor + num_relative])
        cursor += num_relative
        local_indices = set(file_indices[cursor : cursor + num_local])

        # Build circular pairs from the circular set.
        circular_list = sorted(circular_indices)
        circular_pairs: list[tuple[int, int]] = []
        for i in range(0, len(circular_list) - 1, 2):
            circular_pairs.append((circular_list[i], circular_list[i + 1]))

        circular_paired: set[int] = set()
        for a, b in circular_pairs:
            circular_paired.add(a)
            circular_paired.add(b)

        for idx, (rel_path, module_name) in enumerate(group_files):
            lines: list[str] = []

            if idx in circular_paired:
                # Find the partner.
                partner_idx = None
                for a, b in circular_pairs:
                    if idx == a:
                        partner_idx = b
                    elif idx == b:
                        partner_idx = a
                if partner_idx is not None:
                    partner_module = group_files[partner_idx][1]
                    lines.append(f"from {partner_module} import *  # circular")
                lines.append("")
                lines.append(f"VALUE_{idx} = {idx}")
                lines.append("")

            elif idx in stdlib_indices:
                # Pick 1-3 random stdlib imports.
                num_imports = rng.randint(1, 3)
                chosen = rng.sample(STDLIB_MODULES, min(num_imports, len(STDLIB_MODULES)))
                for mod in chosen:
                    lines.append(f"import {mod}")
                lines.append("")
                lines.append(f"VALUE_{idx} = {idx}")
                lines.append("")

            elif idx in relative_indices:
                # Relative imports: from . import sibling or from ..pkg import name.
                pkg_path = "/".join(rel_path.split("/")[:-1])
                short_name = rel_path.split("/")[-1].removesuffix(".py")
                siblings = [
                    sname
                    for sidx, sname in pkg_to_files.get(pkg_path, [])
                    if sidx != idx
                ]
                if siblings:
                    # Pick 1-2 siblings for single-dot relative imports.
                    chosen = rng.sample(siblings, min(rng.randint(1, 2), len(siblings)))
                    for sib in chosen:
                        lines.append(f"from .{sib} import *  # relative")
                else:
                    # No siblings — fall back to a bare relative package import.
                    lines.append("from . import *  # relative (no siblings)")

                # Optionally add a double-dot import (~50% chance) if there are
                # other packages in the group that are actual siblings at the
                # parent level.
                if rng.random() < 0.5:
                    other_pkgs = [
                        p for p in packages if p != pkg_path and p != pkg_path
                    ]
                    if other_pkgs:
                        target_pkg = rng.choice(other_pkgs)
                        # Build relative path from this file's package.
                        target_last = target_pkg.split("/")[-1]
                        lines.append(
                            f"from ..{target_last} import *  # relative double-dot"
                        )

                lines.append("")
                lines.append(f"VALUE_{idx} = {idx}")
                lines.append("")

            elif idx in local_indices:
                # Local (function-scoped) imports.
                candidates = [
                    m
                    for m in available_modules
                    if m != module_name and not m.startswith(module_name + ".")
                ]
                lines.append(f"VALUE_{idx} = {idx}")
                lines.append("")
                lines.append("")
                lines.append(f"def compute_{idx}():")
                if candidates:
                    chosen_mod = rng.choice(candidates)
                    lines.append(
                        f"    from {chosen_mod} import *  # local import"
                    )
                else:
                    lines.append("    import os  # local import fallback")
                lines.append(f"    return VALUE_{idx}")
                lines.append("")

                # Optionally add a second function with a relative local import.
                if rng.random() < 0.5:
                    pkg_path = "/".join(rel_path.split("/")[:-1])
                    siblings = [
                        sname
                        for sidx, sname in pkg_to_files.get(pkg_path, [])
                        if sidx != idx
                    ]
                    if siblings:
                        sib = rng.choice(siblings)
                        lines.append("")
                        lines.append(f"def helper_{idx}():")
                        lines.append(
                            f"    from .{sib} import *  # local relative import"
                        )
                        lines.append(f"    return VALUE_{idx} + 1")
                        lines.append("")

            else:
                # Normal file: import 1-3 other modules (no cycle).
                candidates = [
                    m
                    for m in available_modules
                    if m != module_name and not m.startswith(module_name + ".")
                ]
                if candidates:
                    num_imports = rng.randint(1, min(3, len(candidates)))
                    chosen = rng.sample(candidates, num_imports)
                    for mod in chosen:
                        # Use 'from X import *' style.
                        lines.append(f"from {mod} import *")
                else:
                    lines.append("# standalone module")
                lines.append("")
                lines.append(f"VALUE_{idx} = {idx}")
                lines.append("")

            generated_files[rel_path] = "\n".join(lines)

    return generated_files


# ---------------------------------------------------------------------------
# Main logic
# ---------------------------------------------------------------------------

def generate(scale: int = 1, seed: int = 42) -> None:
    """Generate the sample project."""
    rng = random.Random(seed)

    # Clean output directory.
    if OUTPUT_DIR.exists():
        shutil.rmtree(OUTPUT_DIR)

    # Merge base + scaled files.
    all_files: dict[str, str] = dict(BASE_FILES)
    scaled = _generate_scaled_files(scale, rng)
    all_files.update(scaled)

    # Write files.
    for rel_path, content in sorted(all_files.items()):
        full_path = OUTPUT_DIR / rel_path
        full_path.parent.mkdir(parents=True, exist_ok=True)
        full_path.write_text(content)

    # Write oboros.toml config file.
    # All first-party packages live directly under the project root,
    # so the source root is ".".
    config_path = OUTPUT_DIR / "oboros.toml"
    config_path.write_text(
        'source-roots = ["."]\n'
        "\n"
        "[parse]\n"
        "local-imports = false\n"
        "\n"
        "[cycles]\n"
        "min-scc-size = 2\n"
    )

    # Collect stats.
    total_files = len(all_files)
    init_files = sum(1 for p in all_files if p.endswith("__init__.py"))
    source_files = total_files - init_files
    max_depth = max(p.count("/") for p in all_files) + 1

    # Count circular import markers (rough proxy).
    circular_count = sum(1 for c in all_files.values() if "# circular" in c)
    # Count stdlib imports.
    stdlib_set = set(STDLIB_MODULES)
    files_with_stdlib = 0
    files_with_relative = 0
    files_with_local = 0
    for content in all_files.values():
        has_stdlib = False
        has_relative = False
        has_local = False
        in_function = False
        for line in content.splitlines():
            stripped = line.strip()
            # Detect function/method bodies (rough heuristic).
            if stripped.startswith("def ") and stripped.endswith(":"):
                in_function = True
                continue
            # Lines at indent level 0 leave function scope.
            if line and not line[0].isspace():
                in_function = False
            if stripped.startswith("import ") or stripped.startswith("from "):
                # Check if this import is inside a function.
                if in_function:
                    has_local = True
                # Check for relative import (from . or from ..).
                if stripped.startswith("from ."):
                    has_relative = True
                # Check for stdlib.
                top_module = stripped.split()[1].split(".")[0].lstrip(".")
                if top_module in stdlib_set:
                    has_stdlib = True
        if has_stdlib:
            files_with_stdlib += 1
        if has_relative:
            files_with_relative += 1
        if has_local:
            files_with_local += 1

    print(f"Generated sample project at: {OUTPUT_DIR}")
    print(f"  Scale factor:          {scale}")
    print(f"  Total files:           {total_files}")
    print(f"  Source files:          {source_files}")
    print(f"  __init__.py files:     {init_files}")
    print(f"  Max nesting depth:     {max_depth}")
    print(f"  Files with circular:   {circular_count}")
    print(f"  Files with stdlib:     {files_with_stdlib}")
    print(f"  Files with relative:   {files_with_relative}")
    print(f"  Files with local:      {files_with_local}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate a sample Python project for testing ouroboros.",
    )
    parser.add_argument(
        "--scale",
        type=int,
        default=1,
        help="Scale factor (default: 1 = ~30 files). Each increment adds ~25 files.",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=42,
        help="Random seed for reproducible generation (default: 42).",
    )
    args = parser.parse_args()

    if args.scale < 1:
        parser.error("--scale must be >= 1")

    generate(scale=args.scale, seed=args.seed)


if __name__ == "__main__":
    main()
