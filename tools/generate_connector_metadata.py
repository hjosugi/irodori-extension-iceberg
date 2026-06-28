#!/usr/bin/env python3
"""Generate connector metadata from a thin connector.source.json.

The checked-in connector.config.json and irodori.extension.json files remain
packaging artifacts. connector.source.json is the human-editable source of
truth; this script expands shared defaults, auth method presets, runtime
metadata, source snapshots, and manifest wrappers.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
from collections import OrderedDict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PRESETS_PATH = ROOT / "tools" / "connector_metadata_presets.json"

CONFIG_FILE = "connector.config.json"
MANIFEST_FILE = "irodori.extension.json"
SOURCE_FILE = "connector.source.json"


Json = dict[str, Any]


def ordered(items: list[tuple[str, Any]]) -> Json:
    return OrderedDict(items)


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh, object_pairs_hook=OrderedDict)


def dumps_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def write_json(path: Path, value: Any) -> None:
    path.write_text(dumps_json(value), encoding="utf-8")


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def extension_dirs(root: Path) -> list[Path]:
    if (root / CONFIG_FILE).exists() and (root / MANIFEST_FILE).exists():
        return [root]
    return sorted(
        p
        for p in root.glob("irodori-extension-*")
        if p.is_dir() and (p / CONFIG_FILE).exists() and (p / MANIFEST_FILE).exists()
    )


def source_dirs(root: Path) -> list[Path]:
    if (root / SOURCE_FILE).exists():
        return [root]
    return sorted(p for p in root.glob("irodori-extension-*") if (p / SOURCE_FILE).exists())


def method_preset_key(method: Json) -> str:
    method_id = method["id"]
    if method_id == "oidc" and method.get("label") != "OIDC":
        return "oidc:workloadIdentity"
    return method_id


def tls_preset_key(tls: Json) -> str:
    if not tls.get("supported"):
        return "disabled"
    if tls.get("requiredByDefault"):
        return "required"
    return "optional"


def transports_preset_key(transports: list[str]) -> str:
    if transports == ["direct", "sshTunnel", "socks5Proxy", "httpConnectProxy", "proxyChain"]:
        return "default"
    if transports == ["localFile", "direct"]:
        return "localFileDirect"
    if transports == [
        "customEndpoint",
        "direct",
        "sshTunnel",
        "socks5Proxy",
        "httpConnectProxy",
        "proxyChain",
    ]:
        return "pinecone"
    raise ValueError(f"unknown transports preset: {transports}")


def is_driver_linked(config: Json) -> bool:
    return bool(config["runtime"]["driverLinked"])


def build_presets(dirs: list[Path]) -> Json:
    if not dirs:
        raise ValueError("no extension directories found")

    first_config = load_json(dirs[0] / CONFIG_FILE)
    first_connection = first_config["connector"]["connection"]
    profile_fields = first_connection["profileFields"]

    auth_methods: Json = OrderedDict()
    tls_presets: Json = OrderedDict()
    transports_presets: Json = OrderedDict()
    common_snapshots: list[Json] | None = None

    for ext_dir in dirs:
        config = load_json(ext_dir / CONFIG_FILE)
        connection = config["connector"]["connection"]

        for method in connection["authMethods"]:
            key = method_preset_key(method)
            existing = auth_methods.get(key)
            if existing is not None and existing != method:
                raise ValueError(f"auth method preset collision for {key} in {ext_dir}")
            auth_methods[key] = method

        tls_key = tls_preset_key(connection["tls"])
        tls_presets.setdefault(tls_key, connection["tls"])

        transports_key = transports_preset_key(connection["transports"])
        transports_presets.setdefault(transports_key, connection["transports"])

        source = config["source"]
        snapshots = [
            ordered(
                [
                    ("kind", snap["kind"]),
                    ("path", snap["path"]),
                    ("destination", snap["destination"]),
                ]
            )
            for snap in source["snapshots"]
            if snap["kind"] != "desktop-db-adapter"
        ]
        if common_snapshots is None:
            common_snapshots = snapshots
        elif common_snapshots != snapshots:
            raise ValueError(f"common snapshots differ in {ext_dir}")

    base_config = first_config
    base_manifest = load_json(dirs[0] / MANIFEST_FILE)

    return ordered(
        [
            ("schemaVersion", 1),
            (
                "config",
                ordered(
                    [
                        ("schemaVersion", base_config["schemaVersion"]),
                        ("visibility", base_config["visibility"]),
                    ]
                ),
            ),
            (
                "manifest",
                ordered(
                    [
                        ("$schema", base_manifest["$schema"]),
                        ("manifestVersion", base_manifest["manifestVersion"]),
                        ("publisher", base_manifest["publisher"]),
                        ("license", base_manifest["license"]),
                        ("apiVersion", base_manifest["apiVersion"]),
                        ("runtime", base_manifest["runtime"]),
                        ("entry", base_manifest["entry"]),
                        ("permissions", base_manifest["permissions"]),
                        ("devWatch", base_manifest["dev"]["watch"]),
                    ]
                ),
            ),
            (
                "runtime",
                ordered(
                    [
                        ("abi", base_config["runtime"]["abi"]),
                        ("entrypoints", base_config["runtime"]["entrypoints"]),
                        ("modulePath", base_config["runtime"]["module"]["path"]),
                        ("platforms", base_config["runtime"]["module"]["platforms"]),
                        ("metadataCalls", ["health", "describe", "manifest", "config"]),
                        ("driverCalls", ["connect", "query", "metadata", "close"]),
                    ]
                ),
            ),
            (
                "connection",
                ordered(
                    [
                        ("schemaVersion", first_connection["schemaVersion"]),
                        ("inferEnvironmentFrom", first_connection["inferEnvironmentFrom"]),
                        ("compatibility", first_connection["compatibility"]),
                        ("commonOptionNamespaces", ["profile.options", "driver", "tls", "network"]),
                        ("leadingProfileFields", profile_fields[:3]),
                        ("trailingProfileFields", profile_fields[-2:]),
                    ]
                ),
            ),
            ("authMethods", auth_methods),
            ("tls", tls_presets),
            ("transports", transports_presets),
            (
                "source",
                ordered(
                    [
                        ("commonSnapshots", common_snapshots or []),
                        (
                            "adapterSnapshot",
                            ordered(
                                [
                                    ("kind", "desktop-db-adapter"),
                                    ("pathPrefix", "apps/desktop/src-tauri/src"),
                                    ("destinationPrefix", "native/source/irodori-table/apps/desktop/src-tauri/src"),
                                ]
                            ),
                        ),
                    ]
                ),
            ),
        ]
    )


def source_from_existing(ext_dir: Path, presets: Json) -> Json:
    config = load_json(ext_dir / CONFIG_FILE)
    manifest = load_json(ext_dir / MANIFEST_FILE)
    connector = config["connector"]
    connection = connector["connection"]
    source = config["source"]

    common_prefix = presets["connection"]["commonOptionNamespaces"]
    option_namespaces = connection["optionNamespaces"]
    if option_namespaces[: len(common_prefix)] != common_prefix:
        raise ValueError(f"{ext_dir}: option namespace prefix differs")

    extra_permissions = [
        item for item in manifest["permissions"] if item not in presets["manifest"]["permissions"]
    ]
    extra_dev_watch = [
        item for item in manifest["dev"]["watch"] if item not in presets["manifest"]["devWatch"]
    ]

    generated_source = ordered(
        [
            ("schemaVersion", 1),
            ("extensionId", config["extensionId"]),
            ("visibility", config["visibility"]),
            ("version", manifest["version"]),
            (
                "connector",
                ordered(
                    [
                        ("id", connector["id"]),
                        ("engine", connector["engine"]),
                        ("label", connector["label"]),
                        ("aliases", connector["aliases"]),
                        ("defaultPort", connector["defaultPort"]),
                        ("wire", connector["wire"]),
                        ("module", connector["module"]),
                        ("features", connector["features"]),
                    ]
                ),
            ),
            (
                "connection",
                ordered(
                    [
                        ("endpoint", connection["endpoint"]),
                        ("authMethods", [method_preset_key(method) for method in connection["authMethods"]]),
                        ("tls", tls_preset_key(connection["tls"])),
                        ("transports", transports_preset_key(connection["transports"])),
                        ("optionNamespaces", option_namespaces[len(common_prefix) :]),
                        ("customDriverOptions", connection["customDriverOptions"]),
                    ]
                ),
            ),
            ("runtime", ordered([("driverLinked", is_driver_linked(config))])),
            (
                "source",
                ordered(
                    [
                        ("knowledgeEngineStatus", source["knowledgeEngineStatus"]),
                        ("adapter", source["adapter"]),
                    ]
                ),
            ),
        ]
    )

    if "dialect" in config:
        generated_source["dialect"] = config["dialect"]
    if "experience" in config:
        generated_source["experience"] = config["experience"]
    if extra_permissions or extra_dev_watch:
        manifest_source: Json = OrderedDict()
        if extra_permissions:
            manifest_source["extraPermissions"] = extra_permissions
        if extra_dev_watch:
            manifest_source["extraDevWatch"] = extra_dev_watch
        generated_source["manifest"] = manifest_source
    if "sqlDialects" in manifest.get("contributes", {}):
        manifest_source = generated_source.setdefault("manifest", OrderedDict())
        manifest_source["sqlDialects"] = manifest["contributes"]["sqlDialects"]

    return generated_source


def crate_name(ext_dir: Path) -> str:
    return ext_dir.name.replace("-", "_")


def marketplace_id(source: Json) -> str:
    return source["extensionId"]


def repository_url(ext_dir: Path) -> str:
    return f"https://github.com/hjosugi/{ext_dir.name}"


def profile_fields(source: Json, presets: Json) -> list[Json]:
    endpoint_fields = copy.deepcopy(source["connection"]["endpoint"].get("fields", []))
    return (
        copy.deepcopy(presets["connection"]["leadingProfileFields"])
        + endpoint_fields
        + copy.deepcopy(presets["connection"]["trailingProfileFields"])
    )


def auth_methods(source: Json, presets: Json) -> list[Json]:
    methods = []
    for key in source["connection"]["authMethods"]:
        try:
            methods.append(copy.deepcopy(presets["authMethods"][key]))
        except KeyError as exc:
            raise KeyError(f"unknown auth method preset {key}") from exc
    return methods


def secret_purposes(methods: list[Json]) -> list[str]:
    purposes: list[str] = []
    for method in methods:
        for purpose in method.get("secretPurposes", []):
            if purpose not in purposes:
                purposes.append(purpose)
    return purposes


def source_snapshots(ext_dir: Path, source: Json, presets: Json) -> tuple[str | None, list[Json]]:
    snapshots: list[Json] = []
    adapter = source["source"].get("adapter")
    adapter_sha: str | None = None

    if adapter is not None:
        adapter_preset = presets["source"]["adapterSnapshot"]
        destination = f"{adapter_preset['destinationPrefix']}/{adapter}"
        path = f"{adapter_preset['pathPrefix']}/{adapter}"
        adapter_path = ext_dir / destination
        adapter_sha = sha256_file(adapter_path)
        snapshots.append(
            ordered(
                [
                    ("kind", adapter_preset["kind"]),
                    ("path", path),
                    ("destination", destination),
                    ("sha256", adapter_sha),
                ]
            )
        )

    for snapshot in presets["source"]["commonSnapshots"]:
        destination = snapshot["destination"]
        snapshots.append(
            ordered(
                [
                    ("kind", snapshot["kind"]),
                    ("path", snapshot["path"]),
                    ("destination", destination),
                    ("sha256", sha256_file(ext_dir / destination)),
                ]
            )
        )

    return adapter_sha, snapshots


def build_connection(source: Json, presets: Json) -> Json:
    connector = source["connector"]
    methods = auth_methods(source, presets)
    connection_source = source["connection"]
    return ordered(
        [
            ("schemaVersion", presets["connection"]["schemaVersion"]),
            ("inferEnvironmentFrom", presets["connection"]["inferEnvironmentFrom"]),
            ("compatibility", presets["connection"]["compatibility"]),
            (
                "defaults",
                ordered(
                    [
                        ("engine", connector["engine"]),
                        ("wire", connector["wire"]),
                        ("port", connector["defaultPort"]),
                        ("readOnly", False),
                    ]
                ),
            ),
            ("endpoint", copy.deepcopy(connection_source["endpoint"])),
            ("profileFields", profile_fields(source, presets)),
            ("authMethods", methods),
            ("secretPurposes", secret_purposes(methods)),
            ("tls", copy.deepcopy(presets["tls"][connection_source["tls"]])),
            ("transports", copy.deepcopy(presets["transports"][connection_source["transports"]])),
            (
                "optionNamespaces",
                copy.deepcopy(presets["connection"]["commonOptionNamespaces"])
                + copy.deepcopy(connection_source["optionNamespaces"]),
            ),
            ("customDriverOptions", connection_source["customDriverOptions"]),
        ]
    )


def build_connector(source: Json, connection: Json) -> Json:
    connector_source = source["connector"]
    connector = ordered(
        [
            ("id", connector_source["id"]),
            ("engine", connector_source["engine"]),
            ("label", connector_source["label"]),
            ("aliases", copy.deepcopy(connector_source["aliases"])),
            ("defaultPort", connector_source["defaultPort"]),
            ("wire", connector_source["wire"]),
            ("module", connector_source["module"]),
        ]
    )
    if "dialect" in source:
        connector["dialect"] = source["dialect"]["id"]
    connector["features"] = copy.deepcopy(connector_source["features"])
    connector["connection"] = connection
    if "experience" in source:
        connector["experience"] = copy.deepcopy(source["experience"])
    return connector


def build_runtime(ext_dir: Path, source: Json, presets: Json) -> Json:
    connector = source["connector"]
    runtime_source = source["runtime"]
    calls = list(presets["runtime"]["metadataCalls"])
    if runtime_source["driverLinked"]:
        calls += list(presets["runtime"]["driverCalls"])

    return ordered(
        [
            ("abi", presets["runtime"]["abi"]),
            (
                "module",
                ordered(
                    [
                        ("id", connector["module"]),
                        ("path", presets["runtime"]["modulePath"]),
                        ("platforms", copy.deepcopy(presets["runtime"]["platforms"])),
                    ]
                ),
            ),
            ("crate", crate_name(ext_dir)),
            ("entrypoints", copy.deepcopy(presets["runtime"]["entrypoints"])),
            ("supportedCalls", calls),
            ("driverLinked", runtime_source["driverLinked"]),
        ]
    )


def build_source_block(ext_dir: Path, source: Json, presets: Json) -> Json:
    adapter_sha, snapshots = source_snapshots(ext_dir, source, presets)
    source_input = source["source"]
    return ordered(
        [
            ("marketplaceId", marketplace_id(source)),
            ("repository", repository_url(ext_dir)),
            ("knowledgeEngineStatus", source_input["knowledgeEngineStatus"]),
            ("adapter", source_input.get("adapter")),
            ("adapterSha256", adapter_sha),
            ("snapshots", snapshots),
        ]
    )


def build_config(ext_dir: Path, source: Json, presets: Json) -> Json:
    connection = build_connection(source, presets)
    connector = build_connector(source, connection)
    result = ordered(
        [
            ("schemaVersion", presets["config"]["schemaVersion"]),
            ("visibility", source["visibility"]),
            ("extensionId", source["extensionId"]),
            ("connector", connector),
            ("runtime", build_runtime(ext_dir, source, presets)),
            ("source", build_source_block(ext_dir, source, presets)),
            ("connection", connection),
        ]
    )
    if "dialect" in source:
        result["dialect"] = copy.deepcopy(source["dialect"])
    if "experience" in source:
        result["experience"] = copy.deepcopy(source["experience"])
    return result


def build_manifest(ext_dir: Path, source: Json, config: Json, presets: Json) -> Json:
    connector = source["connector"]
    label = connector["label"]
    manifest_source = source.get("manifest", {})
    permissions = copy.deepcopy(presets["manifest"]["permissions"]) + copy.deepcopy(
        manifest_source.get("extraPermissions", [])
    )
    dev_watch = copy.deepcopy(presets["manifest"]["devWatch"]) + copy.deepcopy(
        manifest_source.get("extraDevWatch", [])
    )
    contributes = OrderedDict()
    if "sqlDialects" in manifest_source:
        contributes["sqlDialects"] = copy.deepcopy(manifest_source["sqlDialects"])
    contributes["connectors"] = [config["connector"]]

    return ordered(
        [
            ("$schema", presets["manifest"]["$schema"]),
            ("manifestVersion", presets["manifest"]["manifestVersion"]),
            ("id", source["extensionId"]),
            ("name", f"{label} Connector"),
            ("version", source["version"]),
            ("publisher", presets["manifest"]["publisher"]),
            (
                "description",
                f"{label} Connector contributes the {label} database connector through the native connector ABI.",
            ),
            ("license", presets["manifest"]["license"]),
            ("repository", repository_url(ext_dir)),
            ("apiVersion", presets["manifest"]["apiVersion"]),
            ("runtime", presets["manifest"]["runtime"]),
            ("entry", presets["manifest"]["entry"]),
            ("permissions", permissions),
            ("contributes", contributes),
            (
                "capabilities",
                ordered(
                    [
                        (
                            "nativeModules",
                            [
                                ordered(
                                    [
                                        ("id", connector["module"]),
                                        ("path", presets["runtime"]["modulePath"]),
                                        ("platforms", copy.deepcopy(presets["runtime"]["platforms"])),
                                    ]
                                )
                            ],
                        )
                    ]
                ),
            ),
            ("dev", ordered([("watch", dev_watch)])),
        ]
    )


def build_outputs(ext_dir: Path, presets: Json) -> tuple[Json, Json]:
    source = load_json(ext_dir / SOURCE_FILE)
    config = build_config(ext_dir, source, presets)
    manifest = build_manifest(ext_dir, source, config, presets)
    return config, manifest


def compare_or_write(path: Path, value: Any, write: bool) -> bool:
    generated = dumps_json(value)
    if write:
        path.write_text(generated, encoding="utf-8")
        return True
    current = path.read_text(encoding="utf-8") if path.exists() else ""
    return current == generated


def command_bootstrap(args: argparse.Namespace) -> int:
    dirs = extension_dirs(ROOT)
    presets = build_presets(dirs)
    if args.write:
        write_json(PRESETS_PATH, presets)
    else:
        print(dumps_json(presets), end="")

    for ext_dir in dirs:
        source = source_from_existing(ext_dir, presets)
        if args.write:
            write_json(ext_dir / SOURCE_FILE, source)
        else:
            print(f"--- {ext_dir / SOURCE_FILE}")
            print(dumps_json(source), end="")
    return 0


def command_generate(args: argparse.Namespace) -> int:
    presets = load_json(PRESETS_PATH)
    dirs = [Path(p) for p in args.extensions] if args.extensions else source_dirs(ROOT)
    failures: list[str] = []

    for ext_dir in dirs:
        config, manifest = build_outputs(ext_dir, presets)
        write = not args.check
        config_ok = compare_or_write(ext_dir / CONFIG_FILE, config, write)
        manifest_ok = compare_or_write(ext_dir / MANIFEST_FILE, manifest, write)
        if args.check and (not config_ok or not manifest_ok):
            if not config_ok:
                failures.append(f"{ext_dir / CONFIG_FILE}")
            if not manifest_ok:
                failures.append(f"{ext_dir / MANIFEST_FILE}")

    if failures:
        for failure in failures:
            print(f"metadata out of date: {failure}", file=sys.stderr)
        return 1
    if args.check:
        print(f"metadata check passed for {len(dirs)} extension(s)")
    else:
        print(f"metadata generated for {len(dirs)} extension(s)")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    bootstrap = subparsers.add_parser(
        "bootstrap",
        help="derive presets and connector.source.json from existing generated metadata",
    )
    bootstrap.add_argument("--write", action="store_true", help="write presets and source files")
    bootstrap.set_defaults(func=command_bootstrap)

    generate = subparsers.add_parser(
        "generate",
        help="generate connector.config.json and irodori.extension.json from connector.source.json",
    )
    generate.add_argument("--check", action="store_true", help="only verify generated files")
    generate.add_argument("extensions", nargs="*", help="optional extension directories")
    generate.set_defaults(func=command_generate)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
