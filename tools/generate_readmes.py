#!/usr/bin/env python3
"""Generate English README files from connector metadata."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONFIG_FILE = "connector.config.json"
MANIFEST_FILE = "irodori.extension.json"
SOURCE_FILE = "connector.source.json"
README_FILE = "README.md"

Json = dict[str, Any]


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def extension_dirs(root: Path) -> list[Path]:
    if (root / CONFIG_FILE).exists() and (root / MANIFEST_FILE).exists():
        return [root]
    return sorted(
        p
        for p in root.glob("irodori-extension-*")
        if p.is_dir() and (p / CONFIG_FILE).exists() and (p / MANIFEST_FILE).exists()
    )


def csv(values: list[Any]) -> str:
    if not values:
        return "none"
    return ", ".join(f"`{value}`" for value in values)


def yes_no(value: bool) -> str:
    return "yes" if value else "no"


def line_table(headers: list[str], rows: list[list[str]]) -> list[str]:
    lines = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join("---" for _ in headers) + " |",
    ]
    lines.extend("| " + " | ".join(row) + " |" for row in rows)
    return lines


def auth_table(auth_methods: list[Json]) -> list[str]:
    rows = []
    for method in auth_methods:
        rows.append(
            [
                f"`{method['id']}`",
                method["label"],
                f"`{method['kind']}`",
                csv(method.get("secretPurposes", [])),
            ]
        )
    return line_table(["Auth method", "Label", "Kind", "Secret purposes"], rows)


def endpoint_fields(fields: list[Json]) -> list[str]:
    if not fields:
        return []
    rows = []
    for field in fields:
        rows.append(
            [
                f"`{field['id']}`",
                field["label"],
                f"`{field['type']}`",
                yes_no(bool(field.get("required"))),
            ]
        )
    return ["", "### Endpoint Fields", "", *line_table(["Field", "Label", "Type", "Required"], rows)]


def experience_section(connector: Json) -> list[str]:
    experience = connector.get("experience")
    if not experience:
        return []

    lines = [
        "",
        "## Experience Metadata",
        "",
        f"- Domains: {csv(experience.get('domains', []))}",
        f"- Result views: {csv(experience.get('resultViews', []))}",
        f"- Object types: {csv(experience.get('objectTypes', []))}",
    ]
    inspired_by = experience.get("inspiredBy", [])
    if inspired_by:
        lines.append(f"- Inspired by: {', '.join(inspired_by)}")

    workflows = experience.get("workflows", [])
    if workflows:
        rows = []
        for workflow in workflows:
            rows.append(
                [
                    workflow["label"],
                    f"`{workflow.get('resultView', '')}`",
                    csv(workflow.get("templateIds", [])),
                ]
            )
        lines.extend(["", *line_table(["Workflow", "Result view", "Templates"], rows)])

    templates = experience.get("queryTemplates", [])
    if templates:
        rows = []
        for template in templates:
            rows.append(
                [
                    f"`{template['id']}`",
                    template["label"],
                    f"`{template.get('language', '')}`",
                    f"`{template.get('resultView', '')}`",
                ]
            )
        lines.extend(["", *line_table(["Template", "Label", "Language", "Result view"], rows)])

    return lines


def dialect_section(config: Json) -> list[str]:
    dialect = config.get("dialect")
    if not dialect:
        return []
    snippets = dialect.get("snippets", [])
    keywords = dialect.get("keywords", [])
    lines = [
        "",
        "## SQL Dialect Metadata",
        "",
        f"- Dialect ID: `{dialect['id']}`",
        f"- Name: {dialect['name']}",
        f"- Aliases: {csv(dialect.get('aliases', []))}",
        f"- Keyword entries: `{len(keywords)}`",
        f"- Snippets: `{len(snippets)}`",
        "",
        "Dialect metadata supports highlighting, completion, snippets, and query templates. It is intentionally lightweight metadata, not a complete SQL parser or syntax tree.",
    ]
    if snippets:
        rows = []
        for snippet in snippets[:12]:
            rows.append([snippet["label"], f"`{snippet.get('kind', 'snippet')}`"])
        lines.extend(["", *line_table(["Snippet", "Kind"], rows)])
    return lines


def call_rows(calls: list[str]) -> list[list[str]]:
    descriptions = {
        "health": "Returns connector health, engine id, ABI version, and driver status.",
        "describe": "Returns the embedded manifest and connector config.",
        "manifest": "Returns raw `irodori.extension.json`.",
        "config": "Returns raw `connector.config.json`.",
        "connect": "Opens and validates a native connector connection.",
        "query": "Runs a connector query and returns structured rows or JSON results.",
        "metadata": "Reads schemas, tables, columns, indexes, collections, or equivalent metadata.",
        "close": "Closes and removes a cached native connection.",
    }
    return [[f"`{call}`", descriptions.get(call, "Handles the native connector call.")] for call in calls]


def build_readme(ext_dir: Path) -> str:
    config = load_json(ext_dir / CONFIG_FILE)
    manifest = load_json(ext_dir / MANIFEST_FILE)
    source = load_json(ext_dir / SOURCE_FILE)

    connector = config["connector"]
    connection = connector["connection"]
    runtime = config["runtime"]
    source_block = config["source"]
    endpoint = connection["endpoint"]
    label = connector["label"]

    lines = [
        f"# {label} Connector",
        "",
        f"Native Irodori Table connector extension for {label}.",
        "",
        "This crate packages the connector metadata, native ABI exports, and driver implementation used by the Irodori extension marketplace.",
        "",
        "## Connector",
        "",
        f"- Extension ID: `{config['extensionId']}`",
        f"- Engine ID: `{connector['engine']}`",
        f"- Wire protocol: `{connector['wire']}`",
        f"- Default port: `{connector['defaultPort']}`",
        f"- Native ABI: `{runtime['abi']}`",
        f"- Driver linked: `{yes_no(runtime['driverLinked'])}`",
        f"- Marketplace visibility: `{config['visibility']}`",
        f"- Package version: `{manifest['version']}`",
        "",
    ]

    adapter = source_block.get("adapter")
    if adapter:
        lines.append(f"The package includes a desktop adapter source snapshot from `{adapter}`.")
    else:
        lines.append("The package uses the connector metadata and native driver directly; no desktop adapter source snapshot is required.")

    lines.extend(
        [
            "",
            "Connector metadata lives in `connector.config.json` and `irodori.extension.json`.",
            "The Rust crate exports the native ABI from `src/lib.rs`, uses `irodori-connector-abi` for shared JSON/buffer helpers, and keeps connector behavior in `src/driver.rs`.",
            "",
            "## Connection Metadata",
            "",
            f"- Endpoint modes: {csv(endpoint.get('modes', []))}",
            f"- Transport modes: {csv(connection.get('transports', []))}",
            f"- TLS supported: `{yes_no(connection['tls']['supported'])}`",
            f"- TLS required by default: `{yes_no(connection['tls']['requiredByDefault'])}`",
            f"- Custom driver options: `{yes_no(connection['customDriverOptions'])}`",
            *endpoint_fields(endpoint.get("fields", [])),
            "",
            "## Authentication",
            "",
            "The connector advertises these authentication modes so clients can render the right credential fields. Driver-specific or provider-specific values can still be passed through `options` when needed.",
            "",
            *auth_table(connection["authMethods"]),
        ]
    )

    lines.extend(experience_section(connector))
    lines.extend(dialect_section(config))

    lines.extend(
        [
            "",
            "## Native ABI Calls",
            "",
            *line_table(["Method", "Response"], call_rows(runtime.get("supportedCalls", []))),
            "",
            "## Development",
            "",
            "All extension crates in this checkout share `../target` so dependencies compile once across sibling repositories.",
            "",
            "```sh",
            "make check",
            "make build",
            "```",
            "",
            "Release packages place platform-specific native artifacts under `dist/native`.",
            "",
        ]
    )

    # Keep the source read so future README generation fails early if the file is missing.
    assert source["extensionId"] == config["extensionId"]
    return "\n".join(lines)


def compare_or_write(path: Path, content: str, write: bool) -> bool:
    if write:
        path.write_text(content, encoding="utf-8")
        return True
    return path.exists() and path.read_text(encoding="utf-8") == content


def command_generate(args: argparse.Namespace) -> int:
    dirs = [Path(p) for p in args.extensions] if args.extensions else extension_dirs(ROOT)
    failures: list[str] = []
    for ext_dir in dirs:
        readme = build_readme(ext_dir)
        ok = compare_or_write(ext_dir / README_FILE, readme, not args.check)
        if args.check and not ok:
            failures.append(str(ext_dir / README_FILE))

    if failures:
        for failure in failures:
            print(f"README out of date: {failure}")
        return 1
    if args.check:
        print(f"README check passed for {len(dirs)} extension(s)")
    else:
        print(f"README generated for {len(dirs)} extension(s)")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="only verify generated README files")
    parser.add_argument("extensions", nargs="*", help="optional extension directories")
    args = parser.parse_args(argv)
    return command_generate(args)


if __name__ == "__main__":
    raise SystemExit(main())
