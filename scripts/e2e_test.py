#!/usr/bin/env python3
"""End-to-end test for the expense report intake app.

Uploads documents to a running instance (local or deployed), waits for
the ingestion pipeline to complete, and verifies the bundle was created
with expected artifacts.

Usage:
    # Against local server (default):
    python3 scripts/e2e_test.py reference/ER4971006_Redacted.pdf

    # Against deployed app behind IAP:
    python3 scripts/e2e_test.py --target https://34.160.32.50.nip.io \
        --iap-client-id CLIENT_ID reference/ER4971006_Redacted.pdf

    # With explicit engine and bundle ID:
    python3 scripts/e2e_test.py --engine vertex-gemini-sdk \
        --bundle-id test-bundle reference/ER4971006_Redacted.pdf
"""

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


def get_cloud_run_token(service_url: str, sa_key_path: str) -> str:
    """Get a Cloud Run identity token using a service account key."""
    from google.oauth2 import service_account
    import google.auth.transport.requests

    creds = service_account.IDTokenCredentials.from_service_account_file(
        sa_key_path,
        target_audience=service_url,
    )
    request = google.auth.transport.requests.Request()
    creds.refresh(request)
    return creds.token


def get_iap_token(client_id: str) -> str:
    """Get an IAP identity token via gcloud."""
    result = subprocess.run(
        ["gcloud", "auth", "print-identity-token", f"--audiences={client_id}"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"ERROR: Failed to get IAP token: {result.stderr.strip()}")
        sys.exit(1)
    return result.stdout.strip()


def build_multipart_body(
    files: list[Path],
    fields: dict[str, str],
    boundary: str,
) -> bytes:
    """Build a multipart/form-data body."""
    parts = []
    for key, value in fields.items():
        parts.append(
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{key}"\r\n\r\n'
            f"{value}\r\n"
        )
    for file_path in files:
        filename = file_path.name
        content = file_path.read_bytes()
        parts.append(
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="documents"; filename="{filename}"\r\n'
            f"Content-Type: application/octet-stream\r\n\r\n"
        )
        parts.append(content)
        parts.append(b"\r\n")
    parts.append(f"--{boundary}--\r\n")

    body = b""
    for part in parts:
        body += part.encode("utf-8") if isinstance(part, str) else part
    return body


def upload_documents(
    base_url: str,
    files: list[Path],
    bundle_id: str | None = None,
    engine: str | None = None,
    headers: dict[str, str] | None = None,
) -> str:
    """Upload documents to the app. Returns the bundle ID from the redirect."""
    import re
    import html as htmlmod

    try:
        import requests as req_lib
    except ImportError:
        print("ERROR: 'requests' library required. Install with: pip install requests")
        sys.exit(1)

    url = f"{base_url.rstrip('/')}/upload"
    data: dict[str, str] = {}
    if bundle_id:
        data["bundle_id"] = bundle_id
    if engine:
        data["engine"] = engine

    upload_files = [
        ("documents", (f.name, f.read_bytes(), "application/octet-stream"))
        for f in files
    ]

    resp = req_lib.post(
        url,
        files=upload_files,
        data=data,
        headers=headers or {},
        allow_redirects=False,
        timeout=300,
    )

    if resp.status_code == 303:
        location = resp.headers.get("Location", "")
        bid = urllib.parse.unquote(location.split("/bundle/")[-1].split("/")[0])
        return bid

    print(f"ERROR: Upload failed with HTTP {resp.status_code}")
    for m in re.finditer(r'<p class="notice">(.*?)</p>', resp.text, re.DOTALL):
        print(f"  Server: {htmlmod.unescape(m.group(1).strip())}")
    sys.exit(1)


def _get(url: str, headers: dict[str, str] | None = None):
    """HTTP GET using requests library."""
    import requests as req_lib
    return req_lib.get(url, headers=headers or {}, timeout=30)


def check_bundle_manifest(
    base_url: str,
    bundle_id: str,
    headers: dict[str, str] | None = None,
) -> dict:
    """Fetch the bundle manifest."""
    url = f"{base_url.rstrip('/')}/bundle/{urllib.parse.quote(bundle_id)}/manifest"
    resp = _get(url, headers)
    if resp.status_code != 200:
        print(f"ERROR: Failed to fetch manifest (HTTP {resp.status_code})")
        sys.exit(1)
    return resp.json()


def check_review_session(
    base_url: str,
    bundle_id: str,
    headers: dict[str, str] | None = None,
) -> dict:
    """Fetch the review session state."""
    url = f"{base_url.rstrip('/')}/bundle/{urllib.parse.quote(bundle_id)}/review-session"
    resp = _get(url, headers)
    if resp.status_code != 200:
        print(f"ERROR: Failed to fetch review session (HTTP {resp.status_code})")
        sys.exit(1)
    return resp.json()


def check_artifact_exists(
    base_url: str,
    bundle_id: str,
    artifact_name: str,
    headers: dict[str, str] | None = None,
) -> bool:
    """Check if a bundle artifact is accessible."""
    url = (
        f"{base_url.rstrip('/')}/bundle/{urllib.parse.quote(bundle_id)}"
        f"/artifact/{urllib.parse.quote(artifact_name)}"
    )
    resp = _get(url, headers)
    return resp.status_code == 200


def main():
    parser = argparse.ArgumentParser(description="E2E test for expense report app")
    parser.add_argument("documents", nargs="+", type=Path, help="Document files to upload")
    parser.add_argument("--target", default="http://localhost:8765", help="Base URL of the app")
    parser.add_argument("--iap-client-id", help="IAP OAuth client ID (for deployed app)")
    parser.add_argument("--sa-key", help="Service account key file for Cloud Run auth")
    parser.add_argument("--cloud-run-url", help="Direct Cloud Run URL (bypasses IAP)")
    parser.add_argument("--bundle-id", help="Explicit bundle ID")
    parser.add_argument("--engine", help="Ingestion engine (builtin or vertex-gemini-sdk)")
    args = parser.parse_args()

    # Validate files exist
    for doc in args.documents:
        if not doc.exists():
            print(f"ERROR: File not found: {doc}")
            sys.exit(1)

    # Build auth headers
    headers: dict[str, str] = {}
    effective_target = args.target
    if args.sa_key and args.cloud_run_url:
        print(f"Authenticating via service account key to Cloud Run...")
        token = get_cloud_run_token(args.cloud_run_url, args.sa_key)
        headers["Authorization"] = f"Bearer {token}"
        effective_target = args.cloud_run_url
    elif args.iap_client_id:
        print(f"Fetching IAP token for client {args.iap_client_id[:20]}...")
        token = get_iap_token(args.iap_client_id)
        headers["Authorization"] = f"Bearer {token}"

    # Step 1: Upload
    file_list = ", ".join(d.name for d in args.documents)
    print(f"\n1. Uploading {len(args.documents)} document(s): {file_list}")
    start = time.time()
    bundle_id = upload_documents(
        effective_target,
        args.documents,
        bundle_id=args.bundle_id,
        engine=args.engine,
        headers=headers,
    )
    elapsed = time.time() - start
    print(f"   OK — bundle '{bundle_id}' created ({elapsed:.1f}s)")

    # Step 2: Check manifest
    print(f"\n2. Checking bundle manifest...")
    manifest = check_bundle_manifest(effective_target, bundle_id, headers=headers)
    doc_count = len(manifest.get("documents", []))
    run_count = len(manifest.get("runs", []))
    print(f"   OK — {doc_count} document(s), {run_count} run(s)")

    # Step 3: Check review session
    print(f"\n3. Checking review session state...")
    session = check_review_session(effective_target, bundle_id, headers=headers)
    filing_status = session.get("filing_status", "unknown")
    print(f"   OK — filing status: {filing_status}")
    if "readiness" in session:
        readiness = session["readiness"]
        print(f"   Readiness: {json.dumps(readiness, indent=2)}")

    # Step 4: Check key artifacts
    print(f"\n4. Checking artifacts...")
    artifacts = ["draft.yaml", "validation.json", "readiness.json", "ledger.json"]
    results = {}
    for artifact in artifacts:
        exists = check_artifact_exists(effective_target, bundle_id, artifact, headers=headers)
        results[artifact] = exists
        status = "OK" if exists else "MISSING"
        print(f"   {status} — {artifact}")

    # Summary
    print(f"\n{'='*50}")
    all_ok = all(results.values()) and run_count >= 1
    if all_ok:
        print(f"PASS: Bundle '{bundle_id}' processed successfully")
        print(f"  View: {effective_target}/bundle/{urllib.parse.quote(bundle_id)}/workbench")
    else:
        print(f"FAIL: Bundle '{bundle_id}' has issues")
        if run_count == 0:
            print("  No pipeline runs completed")
        for artifact, exists in results.items():
            if not exists:
                print(f"  Missing artifact: {artifact}")
        sys.exit(1)


if __name__ == "__main__":
    main()
