#!/usr/bin/env python3
"""Submit an already-uploaded macOS build to App Store review.

Uploading a .pkg to App Store Connect only makes a *build* available; it does
not ship anything. A build reaches customers only once it is attached to an
appStoreVersion record and that version is submitted for review. Because the
release workflow only ever uploaded, builds piled up unsubmitted (as of
2026-07-27 there were nine VALID builds behind a single "1.0" version record).
This script closes that gap, performing the whole sequence over the App Store
Connect API:

  1. find the build for --version and wait for processingState == VALID
  2. declare export compliance (HTTPS/TLS only -> no non-exempt encryption)
  3. find or create the appStoreVersion for --version (platform MAC_OS)
  4. attach the build to it
  5. set "What's New" on every version localization
  6. create a reviewSubmission, add the version as an item, mark it submitted

Idempotent: re-running reuses an existing version record or an unsubmitted
review submission instead of erroring, so a retry after a transient failure is
safe.

Credentials come from the environment (same secrets the workflow already uses):

  APPLE_API_KEY_P8      base64-encoded .p8 private key
  APPLE_API_KEY_ID      key id
  APPLE_API_ISSUER_ID   issuer id

Requires the `cryptography` package (for ES256 JWT signing); no other deps.
"""

import argparse
import base64
import json
import os
import sys
import time
import urllib.error
import urllib.request

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, utils as asym_utils

API = "https://api.appstoreconnect.apple.com"
PLATFORM = "MAC_OS"


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def _token() -> str:
    """Mint a short-lived ES256 JWT for the App Store Connect API."""
    raw_p8 = os.environ["APPLE_API_KEY_P8"]
    try:
        pem = base64.b64decode(raw_p8, validate=True)
    except Exception:
        pem = raw_p8.encode()  # already PEM, not base64-wrapped
    key = serialization.load_pem_private_key(pem, password=None)

    header = {"alg": "ES256", "kid": os.environ["APPLE_API_KEY_ID"], "typ": "JWT"}
    now = int(time.time())
    payload = {
        "iss": os.environ["APPLE_API_ISSUER_ID"],
        "iat": now,
        "exp": now + 900,
        "aud": "appstoreconnect-v1",
    }
    signing_input = f"{_b64url(json.dumps(header).encode())}.{_b64url(json.dumps(payload).encode())}"
    der = key.sign(signing_input.encode(), ec.ECDSA(hashes.SHA256()))
    r, s = asym_utils.decode_dss_signature(der)
    return f"{signing_input}.{_b64url(r.to_bytes(32, 'big') + s.to_bytes(32, 'big'))}"


def call(method: str, path: str, body=None):
    url = path if path.startswith("http") else f"{API}{path}"
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {_token()}")
    if data:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.status, json.loads(resp.read() or b"{}")
    except urllib.error.HTTPError as e:
        raw = e.read() or b"{}"
        try:
            return e.code, json.loads(raw)
        except json.JSONDecodeError:
            return e.code, {"raw": raw.decode(errors="replace")}


def fail(message: str, payload=None):
    print(f"ERROR: {message}", file=sys.stderr)
    if payload is not None:
        print(json.dumps(payload, indent=2)[:2000], file=sys.stderr)
    sys.exit(1)


def errors_of(payload) -> str:
    return "; ".join(
        f"{e.get('title')}: {e.get('detail')}" for e in (payload or {}).get("errors", [])
    ) or json.dumps(payload)[:400]


def wait_for_build(app_id: str, version: str, timeout_s: int, poll_s: int):
    """Return the build id once Apple finishes processing it."""
    deadline = time.time() + timeout_s
    seen_state = None
    while True:
        st, resp = call(
            "GET", f"/v1/builds?filter[app]={app_id}&filter[version]={version}&limit=5"
        )
        if st != 200:
            fail(f"listing builds failed (HTTP {st})", resp)
        for build in resp.get("data", []):
            state = build["attributes"].get("processingState")
            if state != seen_state:
                print(f"  build {build['id']}: {state}")
                seen_state = state
            if state == "VALID":
                return build["id"]
            if state in ("INVALID", "FAILED"):
                fail(f"build {build['id']} is {state} — cannot submit")
        if time.time() >= deadline:
            print(
                f"WARNING: build {version} not VALID within {timeout_s}s "
                f"(last state: {seen_state or 'not found'}). "
                "Re-run this script once processing completes.",
                file=sys.stderr,
            )
            sys.exit(2)
        time.sleep(poll_s)


def find_or_create_version(app_id: str, version: str) -> str:
    st, resp = call(
        "GET",
        f"/v1/apps/{app_id}/appStoreVersions"
        f"?filter[versionString]={version}&filter[platform]={PLATFORM}&limit=5",
    )
    if st == 200 and resp.get("data"):
        existing = resp["data"][0]
        state = existing["attributes"].get("appStoreState") or existing["attributes"].get(
            "appVersionState"
        )
        print(f"  reusing existing version record {existing['id']} (state {state})")
        return existing["id"]

    st, resp = call(
        "POST",
        "/v1/appStoreVersions",
        {
            "data": {
                "type": "appStoreVersions",
                "attributes": {
                    "platform": PLATFORM,
                    "versionString": version,
                    "releaseType": "AFTER_APPROVAL",
                },
                "relationships": {"app": {"data": {"type": "apps", "id": app_id}}},
            }
        },
    )
    if st not in (200, 201):
        fail(f"creating version {version} failed (HTTP {st}): {errors_of(resp)}", resp)
    print(f"  created version record {resp['data']['id']}")
    return resp["data"]["id"]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--app-id", required=True)
    ap.add_argument("--version", required=True, help="marketing version, e.g. 3.1.28")
    ap.add_argument("--whats-new", default="")
    ap.add_argument("--build-timeout", type=int, default=2400, help="seconds")
    ap.add_argument("--poll-interval", type=int, default=60, help="seconds")
    args = ap.parse_args()

    for var in ("APPLE_API_KEY_P8", "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID"):
        if not os.environ.get(var):
            fail(f"{var} is not set")

    whats_new = args.whats_new.strip() or (
        f"rust2xml {args.version}. See "
        f"https://github.com/zdavatz/rust2xml/releases/tag/v{args.version}"
    )

    print(f"Waiting for build {args.version} to finish processing…")
    build_id = wait_for_build(
        args.app_id, args.version, args.build_timeout, args.poll_interval
    )
    print(f"Build ready: {build_id}")

    # Export compliance. rust2xml only performs HTTPS/TLS requests and SHA-256
    # hashing, with no proprietary or non-standard cryptography, so it does not
    # use non-exempt encryption. Without this the submission is blocked.
    st, resp = call(
        "PATCH",
        f"/v1/builds/{build_id}",
        {
            "data": {
                "type": "builds",
                "id": build_id,
                "attributes": {"usesNonExemptEncryption": False},
            }
        },
    )
    if st not in (200, 204):
        fail(f"setting export compliance failed (HTTP {st}): {errors_of(resp)}", resp)
    print("Export compliance declared (no non-exempt encryption)")

    version_id = find_or_create_version(args.app_id, args.version)

    st, resp = call(
        "PATCH",
        f"/v1/appStoreVersions/{version_id}/relationships/build",
        {"data": {"type": "builds", "id": build_id}},
    )
    if st not in (200, 204):
        fail(f"attaching build failed (HTTP {st}): {errors_of(resp)}", resp)
    print("Build attached to version")

    st, locs = call(
        "GET", f"/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=50"
    )
    if st != 200:
        fail(f"listing localizations failed (HTTP {st})", locs)
    for loc in locs.get("data", []):
        st2, resp2 = call(
            "PATCH",
            f"/v1/appStoreVersionLocalizations/{loc['id']}",
            {
                "data": {
                    "type": "appStoreVersionLocalizations",
                    "id": loc["id"],
                    "attributes": {"whatsNew": whats_new},
                }
            },
        )
        locale = loc["attributes"].get("locale")
        if st2 != 200:
            fail(f"setting whatsNew for {locale} failed (HTTP {st2}): {errors_of(resp2)}", resp2)
        print(f"  whatsNew set for {locale}")

    # Reuse an in-flight submission if one exists; Apple allows only one
    # non-terminal reviewSubmission per app+platform.
    submission_id = None
    st, subs = call(
        "GET", f"/v1/reviewSubmissions?filter[app]={args.app_id}&limit=20"
    )
    if st == 200:
        for sub in subs.get("data", []):
            state = sub["attributes"].get("state")
            if sub["attributes"].get("platform") == PLATFORM and state in (
                "READY_FOR_REVIEW",
                "UNRESOLVED_ISSUES",
            ):
                submission_id = sub["id"]
                print(f"  reusing open review submission {submission_id} ({state})")
                break

    if submission_id is None:
        st, resp = call(
            "POST",
            "/v1/reviewSubmissions",
            {
                "data": {
                    "type": "reviewSubmissions",
                    "attributes": {"platform": PLATFORM},
                    "relationships": {
                        "app": {"data": {"type": "apps", "id": args.app_id}}
                    },
                }
            },
        )
        if st not in (200, 201):
            fail(f"creating review submission failed (HTTP {st}): {errors_of(resp)}", resp)
        submission_id = resp["data"]["id"]
        print(f"  created review submission {submission_id}")

    st, resp = call(
        "POST",
        "/v1/reviewSubmissionItems",
        {
            "data": {
                "type": "reviewSubmissionItems",
                "relationships": {
                    "reviewSubmission": {
                        "data": {"type": "reviewSubmissions", "id": submission_id}
                    },
                    "appStoreVersion": {
                        "data": {"type": "appStoreVersions", "id": version_id}
                    },
                },
            }
        },
    )
    if st not in (200, 201):
        # Already attached from an earlier run is not an error worth failing on.
        detail = errors_of(resp)
        if "already" in detail.lower():
            print(f"  version already attached to submission ({detail})")
        else:
            fail(f"adding submission item failed (HTTP {st}): {detail}", resp)
    else:
        print("  version added to review submission")

    st, resp = call(
        "PATCH",
        f"/v1/reviewSubmissions/{submission_id}",
        {
            "data": {
                "type": "reviewSubmissions",
                "id": submission_id,
                "attributes": {"submitted": True},
            }
        },
    )
    if st != 200:
        fail(f"submitting for review failed (HTTP {st}): {errors_of(resp)}", resp)

    attrs = resp["data"]["attributes"]
    print(
        f"SUBMITTED: {args.version} -> state={attrs.get('state')} "
        f"submittedDate={attrs.get('submittedDate')} id={submission_id}"
    )


if __name__ == "__main__":
    main()
