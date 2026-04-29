import unittest
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parent.parent


class DeployConfigTests(unittest.TestCase):
    def test_cloud_run_deploy_keeps_cpu_allocated_for_async_jobs(self):
        config = yaml.safe_load((REPO_ROOT / "deploy" / "cloudbuild.yaml").read_text())
        deploy_step = next(step for step in config["steps"] if step.get("id") == "deploy")
        args = deploy_step.get("args") or []

        self.assertIn("--no-cpu-throttling", args)
        self.assertIn("--min-instances=1", args)
        self.assertIn("--max-instances=1", args)


if __name__ == "__main__":
    unittest.main()
