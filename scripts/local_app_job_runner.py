#!/usr/bin/env python3

import argparse
import importlib.util
from pathlib import Path


def load_local_app_module(repo_root: Path):
    script_path = repo_root / "scripts" / "local_app.py"
    spec = importlib.util.spec_from_file_location("local_app", script_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load local_app from {script_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run one persisted local-app OCR job outside the request handler."
    )
    parser.add_argument("--repo-root", required=True)
    parser.add_argument("--workspace-root", required=True)
    parser.add_argument("--job-id", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    workspace_root = Path(args.workspace_root).resolve()
    local_app = load_local_app_module(repo_root)
    return int(local_app.run_job_once(repo_root, workspace_root, args.job_id))


if __name__ == "__main__":
    raise SystemExit(main())
