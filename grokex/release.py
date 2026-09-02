#!/usr/bin/env python3
"""Build and verify the small Grokex release envelope around Codex binaries."""

import argparse
import gzip
import hashlib
import json
import shutil
import statistics
import tarfile
import tempfile
import tomllib
from pathlib import Path


SOURCE_ROOT = Path(__file__).resolve().parent
RELEASE_SOURCE_PATH = SOURCE_ROOT / "release-source.json"
LIVE_CONTRACTS_PATH = SOURCE_ROOT / "live_contracts.json"


def load_release_source(path: Path = RELEASE_SOURCE_PATH) -> dict[str, str]:
    source = json.loads(path.read_text(encoding="utf-8"))
    version = source["version"]
    if source["upstream_tag"] != f"rust-v{version}":
        raise SystemExit("release-source.json upstream_tag does not match version")
    if len(source["upstream_commit"]) != 40:
        raise SystemExit("release-source.json upstream_commit is not a full SHA")
    return source


def load_live_contracts(path: Path = LIVE_CONTRACTS_PATH) -> dict[str, object]:
    contracts = json.loads(path.read_text(encoding="utf-8"))
    if contracts.get("schema_version") != 1:
        raise SystemExit("live_contracts.json schema_version is unsupported")
    scenarios = contracts.get("scenarios")
    if not isinstance(scenarios, dict) or not scenarios:
        raise SystemExit("live_contracts.json has no scenarios")
    for scenario, contract in scenarios.items():
        if contract.get("required") not in {"always", "seam"}:
            raise SystemExit(f"live contract {scenario} has an invalid required policy")
        deadline = contract.get("turn_deadline_seconds")
        if not isinstance(deadline, int) or isinstance(deadline, bool) or deadline <= 0:
            raise SystemExit(f"live contract {scenario} has an invalid deadline")
        if not isinstance(contract.get("story"), str):
            raise SystemExit(f"live contract {scenario} names no Story")
        if not isinstance(contract.get("seam_paths"), list):
            raise SystemExit(f"live contract {scenario} has no seam paths")
    return contracts


RELEASE_SOURCE = load_release_source()
VERSION = RELEASE_SOURCE["version"]
TAG = f"grokex-v{VERSION}"
UPSTREAM_TAG = RELEASE_SOURCE["upstream_tag"]
UPSTREAM_COMMIT = RELEASE_SOURCE["upstream_commit"]
LIVE_CONTRACTS = load_live_contracts()
STORY_BY_SCENARIO = {
    scenario: contract["story"]
    for scenario, contract in LIVE_CONTRACTS["scenarios"].items()
}
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
SUPPORTED_IMAGE_ARTIFACTS = {
    "image/jpeg": ".jpg",
    "image/png": ".png",
    "image/webp": ".webp",
}
LEGACY_KEYS = {
    "model_provider_adapter",
    "model_provider_registrations",
    "provider_adapter",
    "provider_catalog",
}
LIVE_SCENARIO_ASSERTIONS = {
    "basic-exact-reply": {
        "response_assertion": "nonempty_agent_message",
        "runner_turn_submission_count": 1,
        "status": "completed",
    },
    "encrypted-reasoning-tool-continuation": {
        "encrypted_reasoning_observed": True,
        "history_response_assertion": "exact_match",
        "response_assertion": "exact_match",
        "runner_turn_submission_count": 2,
        "same_thread_history": "completed",
        "status": "completed",
        "tool_continuation": "completed",
    },
    "ultra-full-history-collaboration": {
        "child_completion": "completed",
        "child_model_evidence": "parent_model_default_spawn_and_stock_inheritance",
        "child_model_verified": True,
        "child_parent_link_verified": True,
        "child_provider_binding": "grok/grok-4.6",
        "child_provider_verified": True,
        "child_response_assertion": "canonical_uuid_v4",
        "default_full_history": "completed",
        "evidence_source": "public_snapshot_and_stream",
        "multi_agent_version": "v2",
        "parent_completion": "completed",
        "reasoning_effort": "ultra",
        "response_assertion": "child_echo_match",
        "runner_turn_submission_count": 1,
        "status": "completed",
        "result_delivery": "completed",
        "result_delivery_verified": True,
    },
    "image-generation-history-edit": {
        "edit_agent_reply_seen": True,
        "edit_artifact_match": True,
        "edit_completion": "completed",
        "edit_image_decodable": True,
        "generation_agent_reply_seen": True,
        "generation_artifact_match": True,
        "generation_completion": "completed",
        "generation_image_decodable": True,
        "history_arguments_verified": True,
        "runner_turn_submission_count": 2,
        "same_thread": True,
        "status": "completed",
    },
}


def image_artifact_evidence(value: dict[str, object]) -> dict[str, str]:
    evidence: dict[str, str] = {}
    for phase in ("generation", "edit"):
        mime_key = f"{phase}_image_mime"
        extension_key = f"{phase}_artifact_extension"
        image_mime = value.get(mime_key)
        artifact_extension = value.get(extension_key)
        if (
            not isinstance(image_mime, str)
            or SUPPORTED_IMAGE_ARTIFACTS.get(image_mime) != artifact_extension
        ):
            raise SystemExit("live image artifact codec is invalid")
        evidence[mime_key] = image_mime
        evidence[extension_key] = artifact_extension
    return evidence


LIVE_SCENARIO_DIAGNOSTICS = {
    "basic-exact-reply": (),
    "encrypted-reasoning-tool-continuation": (
        "tool_request_count",
    ),
    "ultra-full-history-collaboration": (
        "child_count",
        "explicit_fork_spawn_count",
        "failed_collaboration_tool_count",
        "missing_spawn_identity_count",
        "provider_response_count",
        "spawn_count",
        "unexpected_collaboration_tool_count",
        "wait_count",
    ),
    "image-generation-history-edit": (
        "image_items_completed",
        "image_items_failed",
    ),
}
if set(LIVE_SCENARIO_ASSERTIONS) != set(STORY_BY_SCENARIO):
    raise SystemExit("live_contracts.json scenarios do not match the validator outcomes")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def archive_name(target: str) -> str:
    return f"{TAG}-{target}.tar.gz"


def normalized_tar_info(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    return info


def write_archive(source: Path, destination: Path) -> None:
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                archive.add(
                    source,
                    arcname=source.name,
                    recursive=True,
                    filter=normalized_tar_info,
                )


def copy_release_files(stage: Path, repository: Path) -> None:
    for relative in (
        "grokex/config.toml.example",
        "grokex/INSTALL.md",
        "grokex/install-grokex.sh",
        "grokex/install-grokex.ps1",
        "LICENSE",
    ):
        shutil.copy2(repository / relative, stage / Path(relative).name)


def package(
    raw_root: Path,
    output: Path,
    repository: Path,
    source_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    for target in targets:
        raw = raw_root / target
        suffix = ".exe" if "windows" in target else ""
        required = (f"codex{suffix}", f"codex-code-mode-host{suffix}")
        for filename in required:
            if not (raw / filename).is_file():
                raise SystemExit(f"missing raw binary for {target}: {filename}")

        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary) / TAG
            bin_dir = stage / "bin"
            bin_dir.mkdir(parents=True)
            copy_release_files(stage, repository)

            shutil.copy2(raw / f"codex{suffix}", bin_dir / f"grokex-bin{suffix}")
            shutil.copy2(
                raw / f"codex-code-mode-host{suffix}",
                bin_dir / f"codex-code-mode-host{suffix}",
            )
            if "windows" in target:
                shutil.copy2(repository / "grokex/grokex.ps1", bin_dir / "grokex.ps1")
            else:
                shutil.copy2(repository / "grokex/grokex", bin_dir / "grokex")
                for executable in (
                    bin_dir / "grokex",
                    bin_dir / "grokex-bin",
                    bin_dir / "codex-code-mode-host",
                    stage / "install-grokex.sh",
                ):
                    executable.chmod(0o755)

            if "linux" in target:
                if not (raw / "bwrap").is_file():
                    raise SystemExit(f"missing raw binary for {target}: bwrap")
                shutil.copy2(raw / "bwrap", bin_dir / "bwrap")
                (bin_dir / "bwrap").chmod(0o755)

            provenance = {
                "archive": archive_name(target),
                "source_sha": source_sha,
                "target": target,
                "upstream_commit": UPSTREAM_COMMIT,
                "version": VERSION,
            }
            (stage / "PROVENANCE.json").write_text(
                json.dumps(provenance, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            write_archive(stage, output / archive_name(target))


def safe_members(archive: tarfile.TarFile) -> list[tarfile.TarInfo]:
    members = archive.getmembers()
    for member in members:
        path = Path(member.name)
        if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk():
            raise SystemExit(f"unsafe archive member: {member.name}")
    return members


def verify_archives(
    dist: Path,
    source_sha: str,
    targets: tuple[str, ...] = TARGETS,
) -> None:
    expected_archives = {archive_name(target) for target in targets}
    actual_archives = {path.name for path in dist.glob("*.tar.gz")}
    if actual_archives != expected_archives:
        raise SystemExit(
            f"archive matrix mismatch: expected {sorted(expected_archives)}, "
            f"got {sorted(actual_archives)}"
        )

    for target in targets:
        path = dist / archive_name(target)
        suffix = ".exe" if "windows" in target else ""
        root = TAG
        required = {
            f"{root}/INSTALL.md",
            f"{root}/LICENSE",
            f"{root}/PROVENANCE.json",
            f"{root}/config.toml.example",
            f"{root}/install-grokex.ps1",
            f"{root}/install-grokex.sh",
            f"{root}/bin/codex-code-mode-host{suffix}",
            f"{root}/bin/grokex-bin{suffix}",
            f"{root}/bin/{'grokex.ps1' if suffix else 'grokex'}",
        }
        if "linux" in target:
            required.add(f"{root}/bin/bwrap")

        with tarfile.open(path, "r:gz") as archive:
            members = safe_members(archive)
            names = {member.name for member in members if member.isfile()}
            missing = required - names
            if missing:
                raise SystemExit(f"{path.name} is missing {sorted(missing)}")
            provenance_file = archive.extractfile(f"{root}/PROVENANCE.json")
            if provenance_file is None:
                raise SystemExit(f"{path.name} has no provenance")
            provenance = json.load(provenance_file)
        expected = {
            "archive": path.name,
            "source_sha": source_sha,
            "target": target,
            "upstream_commit": UPSTREAM_COMMIT,
            "version": VERSION,
        }
        if provenance != expected:
            raise SystemExit(f"{path.name} provenance mismatch")


def find_legacy_key(value: object) -> str | None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in LEGACY_KEYS:
                return key
            found = find_legacy_key(child)
            if found:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_legacy_key(child)
            if found:
                return found
    return None


def verify_profile(path: Path, secret: bool) -> None:
    with path.open("rb") as handle:
        config = tomllib.load(handle)
    legacy = find_legacy_key(config)
    if legacy:
        raise SystemExit(f"profile contains unsupported authority: {legacy}")
    if config.get("model") != "grok-4.6" or config.get("model_provider") != "grok":
        raise SystemExit("profile must select exact grok-4.6 from Provider grok")
    if "model_catalog_json" in config:
        raise SystemExit("profile must use the release-bundled model catalog")
    agents = config.get("agents")
    if isinstance(agents, dict) and agents.get("default_subagent_model") is not None:
        raise SystemExit("profile must not override the default child model")
    providers = config.get("model_providers")
    provider = providers.get("grok") if isinstance(providers, dict) else None
    if not isinstance(provider, dict):
        raise SystemExit("profile has no model_providers.grok table")
    required = {
        "base_url": "https://grok.trustedtunnel.app/v1",
        "wire_api": "grok_responses",
        "requires_openai_auth": False,
        "supports_websockets": False,
    }
    if any(provider.get(key) != value for key, value in required.items()):
        raise SystemExit("profile does not match the supported Grok transport contract")
    env_auth = provider.get("env_key") == "GROK_API_KEY"
    token_auth = secret and bool(provider.get("experimental_bearer_token"))
    if not (env_auth or token_auth):
        raise SystemExit("profile has no supported Grok bearer-token authority")
    if not secret and provider.get("experimental_bearer_token") is not None:
        raise SystemExit("public profile must not contain a bearer token")


VALIDATION_ONLY_PATHS = (
    ".github/workflows/grokex-",
    "grokex/RELEASE_PIPELINE.md",
    "grokex/live_contracts.json",
    "grokex/live_smoke.py",
    "grokex/release.py",
    "grokex/test_live_smoke.py",
    "grokex/test_release.py",
)


def verify_carrier(changed_paths: list[str]) -> None:
    """Require a product-to-carrier diff to touch validation-only paths.

    A carrier commit may fix the validator or a workflow while the product SHA
    it validates stays the ancestor that owns every shipped binary.
    """
    product_paths = [
        path
        for path in changed_paths
        if not any(
            path == allowed or (allowed.endswith("-") and path.startswith(allowed))
            for allowed in VALIDATION_ONLY_PATHS
        )
    ]
    if product_paths:
        raise SystemExit(f"carrier changes product paths: {product_paths}")


def seam_path_matches(seam_path: str, changed_path: str) -> bool:
    if seam_path.endswith("/"):
        return changed_path.startswith(seam_path)
    return changed_path == seam_path


def is_test_only_path(changed_path: str) -> bool:
    """Return whether a path can never change the shipped binary.

    Rust integration tests live under ``tests/`` directories and unit tests in
    ``*_tests.rs`` / ``tests.rs`` modules compiled only under ``cfg(test)``.
    """
    rules: dict[str, list[str]] = LIVE_CONTRACTS.get("test_only_paths", {})
    components = changed_path.split("/")
    if any(
        component in components[:-1]
        for component in rules.get("directory_components", [])
    ):
        return True
    return any(
        changed_path.endswith(suffix) for suffix in rules.get("file_suffixes", [])
    )


def required_scenarios(changed_paths: list[str] | None) -> list[str]:
    """Return the Live scenarios a publication must execute on the exact artifact.

    ``None`` means the diff base is unknown (for example the first publication),
    which requires every scenario. Otherwise a scenario is required when its
    policy is ``always`` or when any changed non-test path touches one of its
    seams or a seam shared by every scenario.
    """
    scenarios: dict[str, dict[str, object]] = LIVE_CONTRACTS["scenarios"]
    if changed_paths is None:
        return list(scenarios)
    changed_paths = [path for path in changed_paths if not is_test_only_path(path)]
    shared: list[str] = LIVE_CONTRACTS.get("all_scenarios_seam_paths", [])
    if any(
        seam_path_matches(seam_path, changed_path)
        for seam_path in shared
        for changed_path in changed_paths
    ):
        return list(scenarios)
    required: list[str] = []
    for scenario, contract in scenarios.items():
        seam_paths: list[str] = contract["seam_paths"]
        if contract["required"] == "always" or any(
            seam_path_matches(seam_path, changed_path)
            for seam_path in seam_paths
            for changed_path in changed_paths
        ):
            required.append(scenario)
    return required


def validate_scenario_evidence(
    value: dict[str, object], scenario: str
) -> dict[str, object]:
    expected_assertions = LIVE_SCENARIO_ASSERTIONS[scenario]
    if any(value.get(key) != expected for key, expected in expected_assertions.items()):
        raise SystemExit(f"live scenario outcome mismatch: {scenario}")
    codec_evidence = (
        image_artifact_evidence(value)
        if scenario == "image-generation-history-edit"
        else {}
    )
    diagnostics: dict[str, int] = {}
    for key in LIVE_SCENARIO_DIAGNOSTICS[scenario]:
        diagnostic = value.get(key)
        if not isinstance(diagnostic, int) or isinstance(diagnostic, bool) or diagnostic < 0:
            raise SystemExit(f"live scenario diagnostic is invalid: {scenario}")
        diagnostics[key] = diagnostic
    return {
        **expected_assertions,
        **codec_evidence,
        **diagnostics,
        "story": STORY_BY_SCENARIO[scenario],
    }


def inherited_scenario_evidence(
    prior: dict[str, object], scenario: str, base_sha: str
) -> dict[str, object]:
    prior_scenarios = prior.get("scenarios")
    if not isinstance(prior_scenarios, dict) or prior.get("source_sha") != base_sha:
        raise SystemExit("prior live evidence does not bind the diff base")
    if prior.get("status") != "completed":
        raise SystemExit("prior live evidence is not a completed proof")
    prior_scenario = prior_scenarios.get(scenario)
    if not isinstance(prior_scenario, dict):
        raise SystemExit(f"prior live evidence has no executed proof for {scenario}")
    validate_scenario_evidence(prior_scenario, scenario)
    prior_runs = prior.get("validation_runs", prior.get("validation_run"))
    return {
        "archive_sha256": prior["archive_sha256"],
        "release_tag": prior.get("release_tag", ""),
        "source_sha": base_sha,
        "story": STORY_BY_SCENARIO[scenario],
        "validation_run": prior_scenario.get("validation_run", prior_runs),
    }


def build_live_evidence(
    evidence_dir: Path,
    archive: Path,
    output: Path,
    source_sha: str,
    validator_sha: str,
    required: list[str] | None = None,
    inherit_from: Path | None = None,
    base_sha: str | None = None,
) -> None:
    """Compose LIVE_EVIDENCE.json from per-scenario release evidence.

    Evidence files may come from several authorized Live runs as long as every
    file binds the same archive digest, source SHA, validator SHA, and contract.
    Scenarios outside ``required`` are inherited from ``inherit_from`` (the
    previously published LIVE_EVIDENCE.json whose source is ``base_sha``).
    """
    archive_digest = sha256(archive)
    contract_digest = sha256(LIVE_CONTRACTS_PATH)
    required_set = set(required) if required is not None else set(LIVE_SCENARIO_ASSERTIONS)
    if not required_set <= set(LIVE_SCENARIO_ASSERTIONS):
        raise SystemExit("required live scenario set is unknown")
    if "basic-exact-reply" not in required_set:
        raise SystemExit("basic-exact-reply is always required")
    common = {
        "archive": archive.name,
        "archive_sha256": archive_digest,
        "catalog": "release-bundled",
        "contract_sha256": contract_digest,
        "mode": "release",
        "model": "grok-4.6",
        "provider": "grok",
        "source_sha": source_sha,
        "validator_sha": validator_sha,
    }
    observed: dict[str, dict[str, object]] = {}
    validation_runs: set[str] = set()
    for path in sorted(evidence_dir.glob("*.json")):
        value = json.loads(path.read_text(encoding="utf-8"))
        scenario = value.get("scenario")
        if scenario not in LIVE_SCENARIO_ASSERTIONS or scenario in observed:
            raise SystemExit("live scenario evidence set is invalid")
        if any(value.get(key) != expected for key, expected in common.items()):
            raise SystemExit(f"live scenario evidence mismatch: {scenario}")
        if value.get("story") != STORY_BY_SCENARIO[scenario]:
            raise SystemExit(f"live scenario Story mismatch: {scenario}")
        run_id = value.get("validation_run")
        if not isinstance(run_id, str) or not run_id:
            raise SystemExit(f"live scenario evidence has no validation run: {scenario}")
        validation_runs.add(run_id)
        observed[scenario] = {
            **validate_scenario_evidence(value, scenario),
            "validation_run": run_id,
        }
    missing = required_set - set(observed)
    if missing:
        raise SystemExit(f"required live scenario evidence is incomplete: {sorted(missing)}")

    inherited: dict[str, dict[str, object]] = {}
    not_executed = set(LIVE_SCENARIO_ASSERTIONS) - set(observed)
    if not_executed:
        if inherit_from is None or base_sha is None:
            raise SystemExit(
                f"live scenario evidence is incomplete without inheritance: {sorted(not_executed)}"
            )
        prior = json.loads(inherit_from.read_text(encoding="utf-8"))
        for scenario in sorted(not_executed):
            inherited[scenario] = inherited_scenario_evidence(prior, scenario, base_sha)

    manifest = {
        "archive": archive.name,
        "archive_sha256": archive_digest,
        "catalog": "release-bundled",
        "contract_sha256": contract_digest,
        "inherited_scenarios": inherited,
        "model": "grok-4.6",
        "provider": "grok",
        "release_tag": TAG,
        "required_scenarios": sorted(required_set),
        "runner_turn_submission_count": sum(
            assertions["runner_turn_submission_count"] for assertions in observed.values()
        ),
        "scenarios": observed,
        "source_sha": source_sha,
        "status": "completed",
        "validation_runs": sorted(validation_runs),
        "validator_sha": validator_sha,
    }
    output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def build_assets(
    archives: Path,
    evidence: Path,
    output: Path,
    repository: Path,
    source_sha: str,
    validation_run: str,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    verify_archives(archives, source_sha)
    for target in TARGETS:
        shutil.copy2(archives / archive_name(target), output / archive_name(target))
    for relative in (
        "grokex/config.toml.example",
        "grokex/install-grokex.sh",
        "grokex/install-grokex.ps1",
    ):
        shutil.copy2(repository / relative, output / Path(relative).name)
    shutil.copy2(evidence, output / "LIVE_EVIDENCE.json")

    manifest = {
        "archives": [archive_name(target) for target in TARGETS],
        "source_sha": source_sha,
        "tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "validation_run": validation_run,
        "version": VERSION,
    }
    (output / "RELEASE.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    checksummed = sorted(path for path in output.iterdir() if path.name != "SHA256SUMS")
    (output / "SHA256SUMS").write_text(
        "".join(f"{sha256(path)}  {path.name}\n" for path in checksummed),
        encoding="utf-8",
    )


def verify_live_identity(
    evidence: dict[str, object],
    source_sha: str,
    validator_sha: str,
    live_runs: list[str],
) -> None:
    required_evidence = {
        "archive": archive_name("x86_64-unknown-linux-musl"),
        "catalog": "release-bundled",
        "contract_sha256": sha256(LIVE_CONTRACTS_PATH),
        "model": "grok-4.6",
        "provider": "grok",
        "release_tag": TAG,
        "source_sha": source_sha,
        "status": "completed",
        "validation_runs": sorted(set(live_runs)),
        "validator_sha": validator_sha,
    }
    if any(evidence.get(key) != value for key, value in required_evidence.items()):
        raise SystemExit("live evidence identity mismatch")


def verify_assets(
    dist: Path,
    source_sha: str,
    validator_sha: str,
    validation_run: str,
    live_runs: list[str],
) -> None:
    expected = {archive_name(target) for target in TARGETS} | {
        "LIVE_EVIDENCE.json",
        "RELEASE.json",
        "SHA256SUMS",
        "config.toml.example",
        "install-grokex.ps1",
        "install-grokex.sh",
    }
    actual = {path.name for path in dist.iterdir() if path.is_file()}
    if actual != expected:
        raise SystemExit(f"release asset mismatch: expected {sorted(expected)}, got {sorted(actual)}")
    verify_archives(dist, source_sha)

    manifest = json.loads((dist / "RELEASE.json").read_text(encoding="utf-8"))
    expected_manifest = {
        "archives": [archive_name(target) for target in TARGETS],
        "source_sha": source_sha,
        "tag": TAG,
        "upstream_commit": UPSTREAM_COMMIT,
        "validation_run": validation_run,
        "version": VERSION,
    }
    if manifest != expected_manifest:
        raise SystemExit("release manifest mismatch")

    evidence = json.loads((dist / "LIVE_EVIDENCE.json").read_text(encoding="utf-8"))
    live_archive = archive_name("x86_64-unknown-linux-musl")
    verify_live_identity(evidence, source_sha, validator_sha, live_runs)
    scenarios = evidence.get("scenarios")
    inherited = evidence.get("inherited_scenarios")
    required = evidence.get("required_scenarios")
    if not isinstance(scenarios, dict) or not isinstance(inherited, dict):
        raise SystemExit("live evidence scenario sets are invalid")
    if not isinstance(required, list) or not set(required) <= set(scenarios):
        raise SystemExit("live evidence required scenarios were not executed")
    if "basic-exact-reply" not in scenarios:
        raise SystemExit("live evidence has no executed basic scenario")
    if set(scenarios) | set(inherited) != set(LIVE_SCENARIO_ASSERTIONS) or set(
        scenarios
    ) & set(inherited):
        raise SystemExit("live evidence scenario set mismatch")
    expected_turns = sum(
        LIVE_SCENARIO_ASSERTIONS[scenario]["runner_turn_submission_count"]
        for scenario in scenarios
    )
    if evidence.get("runner_turn_submission_count") != expected_turns:
        raise SystemExit("live evidence Turn contract is invalid")
    for scenario, actual in scenarios.items():
        if not isinstance(actual, dict):
            raise SystemExit(f"live evidence scenario mismatch: {scenario}")
        validate_scenario_evidence(actual, scenario)
        if actual.get("validation_run") not in live_runs:
            raise SystemExit(f"live evidence scenario run is unknown: {scenario}")
    for scenario, actual in inherited.items():
        if not isinstance(actual, dict) or actual.get("story") != STORY_BY_SCENARIO[scenario]:
            raise SystemExit(f"live evidence inherited Story mismatch: {scenario}")
        if not all(
            isinstance(actual.get(key), str) and actual.get(key)
            for key in ("archive_sha256", "source_sha", "validation_run")
        ):
            raise SystemExit(f"live evidence inherited identity is incomplete: {scenario}")
    if evidence.get("archive_sha256") != sha256(dist / live_archive):
        raise SystemExit("live evidence archive checksum mismatch")

    checksum_lines = (dist / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
    recorded: dict[str, str] = {}
    for line in checksum_lines:
        digest, filename = line.split("  ", 1)
        recorded[filename] = digest
    if set(recorded) != expected - {"SHA256SUMS"}:
        raise SystemExit("checksum manifest file set mismatch")
    for filename, digest in recorded.items():
        if sha256(dist / filename) != digest:
            raise SystemExit(f"checksum mismatch: {filename}")


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, round(fraction * (len(ordered) - 1))))
    return ordered[index]


def summarize_observations(evidence_dir: Path) -> dict[str, object]:
    """Aggregate observation-mode evidence into per-scenario latency statistics.

    Observation evidence never proves a Story; the summary only informs
    deadline reviews with p50/p95/max Turn durations against the contract.
    """
    by_scenario: dict[str, dict[str, object]] = {}
    for path in sorted(evidence_dir.rglob("*.json")):
        value = json.loads(path.read_text(encoding="utf-8"))
        scenario = value.get("scenario")
        if scenario not in LIVE_SCENARIO_ASSERTIONS or value.get("mode") != "observation":
            continue
        entry = by_scenario.setdefault(
            scenario,
            {"outcomes": {}, "runs": 0, "turn_durations_seconds": []},
        )
        entry["runs"] += 1
        outcome = value.get("outcome", "completed" if value.get("status") == "completed" else "unknown")
        outcomes: dict[str, int] = entry["outcomes"]
        outcomes[outcome] = outcomes.get(outcome, 0) + 1
        durations = value.get("turn_durations_seconds")
        if isinstance(durations, list):
            entry["turn_durations_seconds"].extend(
                float(duration) for duration in durations if isinstance(duration, (int, float))
            )
    summary: dict[str, object] = {}
    for scenario, entry in sorted(by_scenario.items()):
        durations: list[float] = entry["turn_durations_seconds"]
        deadline = LIVE_CONTRACTS["scenarios"][scenario]["turn_deadline_seconds"]
        statistics_block: dict[str, object] = {"samples": len(durations)}
        if durations:
            statistics_block.update(
                {
                    "max": round(max(durations), 3),
                    "p50": round(statistics.median(durations), 3),
                    "p95": round(percentile(durations, 0.95), 3),
                    "over_deadline": sum(1 for duration in durations if duration > deadline),
                }
            )
        summary[scenario] = {
            "outcomes": entry["outcomes"],
            "runs": entry["runs"],
            "turn_deadline_seconds": deadline,
            "turn_seconds": statistics_block,
        }
    return summary


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    identity_parser = subparsers.add_parser("identity")
    identity_parser.add_argument("--github-output", action="store_true")

    required_parser = subparsers.add_parser("required-scenarios")
    required_parser.add_argument("--changed-paths", type=Path)
    required_parser.add_argument("--all", action="store_true")

    carrier_parser = subparsers.add_parser("verify-carrier")
    carrier_parser.add_argument("--changed-paths", type=Path, required=True)

    package_parser = subparsers.add_parser("package")
    package_parser.add_argument("--raw-root", type=Path, required=True)
    package_parser.add_argument("--output", type=Path, required=True)
    package_parser.add_argument("--repository", type=Path, required=True)
    package_parser.add_argument("--source-sha", required=True)
    package_parser.add_argument("--target", action="append", choices=TARGETS)

    verify_archives_parser = subparsers.add_parser("verify-archives")
    verify_archives_parser.add_argument("--dist", type=Path, required=True)
    verify_archives_parser.add_argument("--source-sha", required=True)
    verify_archives_parser.add_argument("--target", action="append", choices=TARGETS)

    profile_parser = subparsers.add_parser("verify-profile")
    profile_parser.add_argument("--path", type=Path, required=True)
    profile_parser.add_argument("--secret", action="store_true")

    evidence_parser = subparsers.add_parser("build-live-evidence")
    evidence_parser.add_argument("--evidence-dir", type=Path, required=True)
    evidence_parser.add_argument("--archive", type=Path, required=True)
    evidence_parser.add_argument("--output", type=Path, required=True)
    evidence_parser.add_argument("--source-sha", required=True)
    evidence_parser.add_argument("--validator-sha", required=True)
    evidence_parser.add_argument("--required", action="append")
    evidence_parser.add_argument("--inherit-from", type=Path)
    evidence_parser.add_argument("--base-sha")

    assets_parser = subparsers.add_parser("build-assets")
    assets_parser.add_argument("--archives", type=Path, required=True)
    assets_parser.add_argument("--evidence", type=Path, required=True)
    assets_parser.add_argument("--output", type=Path, required=True)
    assets_parser.add_argument("--repository", type=Path, required=True)
    assets_parser.add_argument("--source-sha", required=True)
    assets_parser.add_argument("--validation-run", required=True)

    verify_assets_parser = subparsers.add_parser("verify-assets")
    verify_assets_parser.add_argument("--dist", type=Path, required=True)
    verify_assets_parser.add_argument("--source-sha", required=True)
    verify_assets_parser.add_argument("--validator-sha", required=True)
    verify_assets_parser.add_argument("--validation-run", required=True)
    verify_assets_parser.add_argument("--live-run", action="append", required=True)

    observations_parser = subparsers.add_parser("summarize-observations")
    observations_parser.add_argument("--evidence-dir", type=Path, required=True)

    args = parser.parse_args()
    if args.command == "identity":
        identity = {
            "release_tag": TAG,
            "upstream_commit": UPSTREAM_COMMIT,
            "upstream_tag": UPSTREAM_TAG,
            "version": VERSION,
        }
        if args.github_output:
            print("".join(f"{key}={value}\n" for key, value in identity.items()), end="")
        else:
            print(json.dumps(identity, indent=2, sort_keys=True))
    elif args.command == "required-scenarios":
        if args.all == (args.changed_paths is not None):
            raise SystemExit("required-scenarios needs exactly one of --all or --changed-paths")
        changed = (
            None
            if args.all
            else [
                line.strip()
                for line in args.changed_paths.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
        )
        print("\n".join(required_scenarios(changed)))
    elif args.command == "verify-carrier":
        verify_carrier(
            [
                line.strip()
                for line in args.changed_paths.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
        )
    elif args.command == "package":
        package(
            args.raw_root,
            args.output,
            args.repository,
            args.source_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-archives":
        verify_archives(
            args.dist,
            args.source_sha,
            tuple(args.target) if args.target else TARGETS,
        )
    elif args.command == "verify-profile":
        verify_profile(args.path, args.secret)
    elif args.command == "build-live-evidence":
        build_live_evidence(
            args.evidence_dir,
            args.archive,
            args.output,
            args.source_sha,
            args.validator_sha,
            args.required,
            args.inherit_from,
            args.base_sha,
        )
    elif args.command == "build-assets":
        build_assets(
            args.archives,
            args.evidence,
            args.output,
            args.repository,
            args.source_sha,
            args.validation_run,
        )
    elif args.command == "verify-assets":
        verify_assets(
            args.dist,
            args.source_sha,
            args.validator_sha,
            args.validation_run,
            args.live_run,
        )
    elif args.command == "summarize-observations":
        print(json.dumps(summarize_observations(args.evidence_dir), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
