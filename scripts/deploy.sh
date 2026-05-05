#!/usr/bin/env bash
# Submit a Cloud Build that runs the test gates → builds the Docker image →
# pushes it → deploys to Cloud Run. Resolves COMMIT_SHA from the current
# short HEAD so the image is tagged with the source it was built from.
#
# There's no GitHub push trigger configured in soe-agile-agents (despite
# what an older comment in deploy/cloudbuild.yaml implied), so deploy is
# a manual gesture: commit, push, then run this script.
#
# Usage:
#   scripts/deploy.sh              # deploys HEAD
#   scripts/deploy.sh <sha>        # deploys an explicit short SHA tag
#                                  # (the source uploaded is still HEAD's
#                                  # working tree — the SHA is just the tag)

set -euo pipefail

PROJECT_ID="soe-agile-agents"
CONFIG="deploy/cloudbuild.yaml"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if [ $# -ge 1 ]; then
    sha="$1"
else
    sha="$(git rev-parse --short HEAD)"
fi

echo "submitting Cloud Build for commit ${sha} to project ${PROJECT_ID} ..."
gcloud builds submit \
    --config="${CONFIG}" \
    --project="${PROJECT_ID}" \
    --substitutions="COMMIT_SHA=${sha}"
