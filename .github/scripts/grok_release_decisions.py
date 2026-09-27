#!/usr/bin/env python3

import argparse
import json
import sys


APP_SERVER_SCHEMA_PREFIXES = (
    "codex-rs/app-server-protocol/",
    "codex-rs/protocol/src/",
    "sdk/python/src/openai_codex/generated/",
)


def merged_pr_number(pull_requests: list[dict], branch: str) -> int:
    candidates = [
        pull_request
        for pull_request in pull_requests
        if pull_request["base"]["ref"] == branch
        and pull_request.get("merged_at") is not None
    ]
    if not candidates:
        raise ValueError("no merged pull request targets the version line")
    return max(candidates, key=lambda pull_request: pull_request["merged_at"])["number"]


def successful_pr_run_id(workflow_runs: dict, head_ref: str) -> int:
    candidates = [
        run
        for run in workflow_runs["workflow_runs"]
        if run["head_branch"] == head_ref
        and run["status"] == "completed"
        and run["conclusion"] == "success"
    ]
    if not candidates:
        raise ValueError("no successful pull-request workflow exists for the candidate")
    return max(candidates, key=lambda run: run["created_at"])["id"]


def cargo_proof_succeeded(jobs: dict) -> bool:
    successful_cargo_jobs = [
        job
        for job in jobs["jobs"]
        if job["name"] == "Cargo"
        and job["status"] == "completed"
        and job["conclusion"] == "success"
    ]
    return len(successful_cargo_jobs) == 1


def app_server_schema_closure_required(paths: list[str]) -> bool:
    return any(path.startswith(APP_SERVER_SCHEMA_PREFIXES) for path in paths)


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="decision", required=True)

    merged_pr = subparsers.add_parser("merged-pr")
    merged_pr.add_argument("--branch", required=True)

    successful_run = subparsers.add_parser("successful-pr-run")
    successful_run.add_argument("--head-ref", required=True)

    subparsers.add_parser("cargo-proof")
    subparsers.add_parser("app-server-schema-closure")

    args = parser.parse_args()
    try:
        if args.decision == "merged-pr":
            print(merged_pr_number(json.load(sys.stdin), args.branch))
        elif args.decision == "successful-pr-run":
            print(successful_pr_run_id(json.load(sys.stdin), args.head_ref))
        elif args.decision == "cargo-proof":
            if not cargo_proof_succeeded(json.load(sys.stdin)):
                raise ValueError("Cargo did not complete successfully exactly once")
        elif args.decision == "app-server-schema-closure":
            paths = [line for line in sys.stdin.read().splitlines() if line]
            print("true" if app_server_schema_closure_required(paths) else "false")
    except (KeyError, TypeError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
