#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import yaml

META_KEYS = {
    "type",
    "required",
    "source",
    "depends_on",
    "allowed_values",
    "description",
    "infer_from",
    "default",
    "validation",
    "fields",
    "items",
}

ITEM_SEGMENT = "__item__"
RUST_KEYWORDS = {
    "Self",
    "abstract",
    "as",
    "async",
    "await",
    "become",
    "box",
    "break",
    "const",
    "continue",
    "crate",
    "do",
    "dyn",
    "else",
    "enum",
    "extern",
    "false",
    "final",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "macro",
    "match",
    "mod",
    "move",
    "mut",
    "override",
    "priv",
    "pub",
    "ref",
    "return",
    "self",
    "static",
    "struct",
    "super",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "union",
    "unsafe",
    "unsized",
    "use",
    "virtual",
    "where",
    "while",
    "yield",
}


@dataclass
class SchemaNode:
    name: str
    path: tuple[str, ...]
    node_type: str
    required: bool = False
    required_expression: str | None = None
    source: str | None = None
    depends_on: list[str] = field(default_factory=list)
    allowed_values: list[str] = field(default_factory=list)
    description: str | None = None
    infer_from: str | None = None
    default: Any = None
    validation: str | None = None
    fields: list["SchemaNode"] = field(default_factory=list)
    item: "SchemaNode | None" = None
    implicit_object: bool = False
    effective_source: str | None = None

    @property
    def is_leaf(self) -> bool:
        return self.node_type not in {"object", "array"}

    @property
    def path_string(self) -> str:
        parts: list[str] = []
        for segment in self.path:
            if segment == ITEM_SEGMENT:
                if not parts:
                    parts.append("[]")
                else:
                    parts[-1] = f"{parts[-1]}[]"
            else:
                parts.append(segment)
        return ".".join(parts)


def load_schema(schema_path: Path) -> dict[str, Any]:
    with schema_path.open("r", encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def parse_required(value: Any) -> tuple[bool, str | None]:
    if value is True:
        return True, None
    if value is False or value is None:
        return False, None
    if isinstance(value, str):
        return False, value
    raise TypeError(f"Unsupported required value: {value!r}")


def child_field_map(raw: dict[str, Any]) -> dict[str, Any]:
    if "fields" in raw:
        fields = raw["fields"]
        if not isinstance(fields, dict):
            raise TypeError(f"Expected mapping in 'fields', got {type(fields).__name__}")
        return fields
    return {key: value for key, value in raw.items() if key not in META_KEYS}


def parse_node(name: str, raw: dict[str, Any], path: tuple[str, ...]) -> SchemaNode:
    if not isinstance(raw, dict):
        raise TypeError(f"Schema node at {'.'.join(path)} must be a mapping")

    explicit_type = raw.get("type")
    has_fields = "fields" in raw
    inline_children = [key for key in raw if key not in META_KEYS]
    required, required_expression = parse_required(raw.get("required"))
    implicit_object = explicit_type is None and bool(inline_children) and not has_fields

    if explicit_type == "array":
        items_raw = raw.get("items")
        if not isinstance(items_raw, dict):
            raise TypeError(f"Array node at {'.'.join(path)} must define mapping 'items'")
        item_node = parse_node(ITEM_SEGMENT, items_raw, path + (ITEM_SEGMENT,))
        return SchemaNode(
            name=name,
            path=path,
            node_type="array",
            required=required,
            required_expression=required_expression,
            source=raw.get("source"),
            depends_on=list(raw.get("depends_on", [])),
            allowed_values=list(raw.get("allowed_values", [])),
            description=raw.get("description"),
            infer_from=raw.get("infer_from"),
            default=raw.get("default"),
            validation=raw.get("validation"),
            item=item_node,
        )

    if explicit_type == "object" or has_fields or inline_children:
        children = [
            parse_node(child_name, child_raw, path + (child_name,))
            for child_name, child_raw in child_field_map(raw).items()
        ]
        return SchemaNode(
            name=name,
            path=path,
            node_type="object",
            required=required or implicit_object,
            required_expression=required_expression,
            source=raw.get("source"),
            depends_on=list(raw.get("depends_on", [])),
            allowed_values=list(raw.get("allowed_values", [])),
            description=raw.get("description"),
            infer_from=raw.get("infer_from"),
            default=raw.get("default"),
            validation=raw.get("validation"),
            fields=children,
            implicit_object=implicit_object,
        )

    if explicit_type is None:
        raise TypeError(f"Leaf node at {'.'.join(path)} is missing 'type'")

    return SchemaNode(
        name=name,
        path=path,
        node_type=explicit_type,
        required=required,
        required_expression=required_expression,
        source=raw.get("source"),
        depends_on=list(raw.get("depends_on", [])),
        allowed_values=list(raw.get("allowed_values", [])),
        description=raw.get("description"),
        infer_from=raw.get("infer_from"),
        default=raw.get("default"),
        validation=raw.get("validation"),
    )


def camelize(value: str) -> str:
    if value == ITEM_SEGMENT:
        return "Item"
    parts = [part for part in re.split(r"[^A-Za-z0-9]+", value) if part]
    if not parts:
        return "Value"
    return "".join(part[:1].upper() + part[1:] for part in parts)


def titleize(value: str) -> str:
    words = [part for part in re.split(r"[_\W]+", value) if part]
    return " ".join(word.capitalize() for word in words) if words else value


def rust_identifier(name: str) -> str:
    identifier = re.sub(r"\W", "_", name)
    if not identifier:
        identifier = "field"
    if identifier[0].isdigit():
        identifier = f"field_{identifier}"
    if identifier in RUST_KEYWORDS:
        identifier = f"r#{identifier}"
    return identifier


def class_name(node: SchemaNode) -> str:
    return "".join(camelize(segment) for segment in node.path)


def enum_alias_name(node: SchemaNode) -> str:
    return f"{class_name(node)}Enum"


def leaf_nodes(node: SchemaNode) -> list[SchemaNode]:
    if node.is_leaf:
        return [node]
    if node.node_type == "array":
        return leaf_nodes(node.item) if node.item else []
    leaves: list[SchemaNode] = []
    for child in node.fields:
        leaves.extend(leaf_nodes(child))
    return leaves


def object_nodes(node: SchemaNode) -> list[SchemaNode]:
    nodes: list[SchemaNode] = []
    if node.node_type == "array" and node.item is not None:
        nodes.extend(object_nodes(node.item))
    if node.node_type == "object":
        for child in node.fields:
            nodes.extend(object_nodes(child))
        nodes.append(node)
    return nodes


def rust_variant_name(value: str) -> str:
    parts = [part for part in re.split(r"[^A-Za-z0-9]+", value) if part]
    if not parts:
        candidate = "Value"
    else:
        candidate = "".join(part[:1].upper() + part[1:] for part in parts)
    if candidate[0].isdigit():
        candidate = f"Value{candidate}"
    if candidate in RUST_KEYWORDS or candidate in {"Self"}:
        candidate = f"{candidate}Value"
    return candidate


def rust_type(node: SchemaNode) -> str:
    if node.node_type == "string":
        return "String"
    if node.node_type == "date":
        return "IsoDate"
    if node.node_type == "number":
        # f64 has ~15-17 decimal digits of precision — sufficient for this
        # domain (travel-expense amounts). The YAML literally says "number";
        # earlier codegen used a string newtype to dodge IEEE 754 precision,
        # which mattered more for accounting at scale than for receipts.
        return "f64"
    if node.node_type == "boolean":
        return "bool"
    if node.node_type == "enum":
        return enum_alias_name(node)
    if node.node_type == "object":
        return class_name(node)
    if node.node_type == "array":
        item_type = rust_type(node.item) if node.item is not None else "String"
        return f"Vec<{item_type}>"
    raise ValueError(f"Unsupported schema type: {node.node_type}")


def rust_field_type(node: SchemaNode) -> str:
    base_type = rust_type(node)
    if node.is_leaf:
        # Wrap by source tier:
        # - T3 leaves are extracted from documents — provenance comes from
        #   the receipt content (quote + page + confidence). → `Wrapped<T>`.
        # - T2 leaves are system-derived (totals, exchange rates, day
        #   counts) — provenance comes from the reduction step that
        #   computed them. Same `Wrapped<T>` shape, with system_generated
        #   evidence and a confidence floor inherited from inputs.
        # - T1 leaves are FA-input — provenance is "the FA typed it" and
        #   doesn't need a structured _meta block. → `Option<T>`.
        # - Leaves with no source tier (root-level / structural) fall back
        #   to the required-ness rule.
        if node.effective_source in ("T2", "T3"):
            return f"Wrapped<{base_type}>"
        if node.effective_source == "T1":
            return f"Option<{base_type}>"
        if node.required and node.required_expression is None:
            return base_type
        return f"Option<{base_type}>"
    if node.required and node.required_expression is None:
        return base_type
    return f"Option<{base_type}>"


def wrap_comment(prefix: str, text: str, width: int = 96) -> list[str]:
    words = text.split()
    if not words:
        return []
    lines: list[str] = []
    current = prefix
    for word in words:
        candidate = f"{current} {word}" if current.strip("# ").strip() else f"{prefix}{word}"
        if len(candidate) > width and current != prefix:
            lines.append(current)
            current = f"{prefix}{word}"
        else:
            current = candidate
    lines.append(current)
    return lines


def node_doc_lines(node: SchemaNode, indent: str = "") -> list[str]:
    lines: list[str] = []
    if node.description:
        lines.extend(wrap_comment(f"{indent}/// ", node.description))
    if node.required_expression:
        lines.extend(wrap_comment(f"{indent}/// Conditionally required when: ", node.required_expression))
    if node.depends_on:
        lines.extend(wrap_comment(f"{indent}/// Depends on: ", ", ".join(node.depends_on)))
    if node.effective_source:
        lines.append(f"{indent}/// Source tier: {node.effective_source}")
    if node.infer_from:
        lines.extend(wrap_comment(f"{indent}/// Infer from: ", node.infer_from))
    if node.validation:
        lines.extend(wrap_comment(f"{indent}/// Validation rule: ", node.validation))
    return lines


def generate_rust_model(root: SchemaNode, schema_version: str) -> str:
    lines: list[str] = [
        "// Auto-generated typed model from schema.yaml. Do not edit manually.",
        "",
        "use serde::{Deserialize, Serialize};",
        "",
        "use crate::meta::Wrapped;",
        "",
        f'pub const SCHEMA_VERSION: &str = "{schema_version}";',
        "",
        "#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]",
        "#[serde(transparent)]",
        "pub struct IsoDate(pub String);",
        "",
        "impl From<String> for IsoDate {",
        "    fn from(value: String) -> Self {",
        "        Self(value)",
        "    }",
        "}",
        "",
        "impl From<&str> for IsoDate {",
        "    fn from(value: &str) -> Self {",
        "        Self(value.to_owned())",
        "    }",
        "}",
        "",
    ]

    enum_nodes = [node for node in leaf_nodes(root) if node.node_type == "enum"]
    if enum_nodes:
        for node in enum_nodes:
            enum_name = enum_alias_name(node)
            lines.extend(node_doc_lines(node))
            lines.append(
                "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]"
            )
            lines.append("#[serde(rename_all = \"snake_case\")]")
            lines.append(f"pub enum {enum_name} {{")
            for index, value in enumerate(node.allowed_values):
                # Default to the first variant. The default value is
                # arbitrary — reduction always either overwrites the value
                # or wraps it with Wrapped::unknown() (value: None). The
                # workbench/validator key off `value.is_some()`.
                if index == 0:
                    lines.append("    #[default]")
                lines.append(f"    {rust_variant_name(value)},")
            lines.append("}")
            lines.append("")
            lines.append(f"impl {enum_name} {{")
            lines.append("    pub const fn as_str(&self) -> &'static str {")
            lines.append("        match self {")
            for value in node.allowed_values:
                lines.append(f"            Self::{rust_variant_name(value)} => {json.dumps(value)},")
            lines.append("        }")
            lines.append("    }")
            lines.append("}")
            lines.append("")
            lines.append(f"impl core::str::FromStr for {enum_name} {{")
            lines.append("    type Err = &'static str;")
            lines.append("")
            lines.append("    fn from_str(value: &str) -> Result<Self, Self::Err> {")
            lines.append("        match value {")
            for value in node.allowed_values:
                lines.append(f"            {json.dumps(value)} => Ok(Self::{rust_variant_name(value)}),")
            lines.append('            _ => Err("invalid enum value"),')
            lines.append("        }")
            lines.append("    }")
            lines.append("}")
            lines.append("")
            lines.append(f"impl core::fmt::Display for {enum_name} {{")
            lines.append("    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {")
            lines.append("        f.write_str(self.as_str())")
            lines.append("    }")
            lines.append("}")
            lines.append("")

    for node in object_nodes(root):
        lines.extend(node_doc_lines(node))
        lines.append("#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]")
        lines.append(f"pub struct {class_name(node)} {{")
        if not node.fields:
            lines.append("}")
            lines.append("")
            continue

        for child in node.fields:
            lines.extend(node_doc_lines(child, indent="    "))
            identifier = rust_identifier(child.name)
            field_type = rust_field_type(child)
            # `serde(default)` lets Python omit fields without breaking deserialize.
            # `skip_serializing_if` lets Rust omit defaulted fields on the way back
            # out so re-serialization round-trips losslessly (M6.5 round-trip check).
            if field_type.startswith("Option<"):
                lines.append('    #[serde(default, skip_serializing_if = "Option::is_none")]')
            elif field_type.startswith("Vec<"):
                lines.append('    #[serde(default, skip_serializing_if = "Vec::is_empty")]')
            elif child.is_leaf:
                # Wrapped<T> — default is Wrapped::unknown() (value: None).
                # We don't skip these; provenance metadata distinguishes
                # "extractor saw nothing" from "field absent."
                lines.append("    #[serde(default)]")
            lines.append(f"    pub {identifier}: {field_type},")
        lines.append("")
        lines.append("}")
        lines.append("")

    lines.append(f"pub type ExpenseReportModel = {class_name(root)};")
    lines.append("")
    return "\n".join(lines)


def node_kind(node: SchemaNode) -> str:
    if node.node_type == "object":
        return "object"
    if node.node_type == "array":
        return "array"
    return "field"


def control_type(node: SchemaNode) -> str:
    if node.node_type == "enum":
        return "select"
    if node.node_type == "boolean":
        return "checkbox"
    if node.node_type == "date":
        return "date"
    if node.node_type == "number":
        amount_like = any(
            token in node.path_string
            for token in ("amount", "rate", "total", "fare", "deduction", "net_amount")
        )
        return "currency" if amount_like else "number"
    multiline = any(
        token in node.path_string
        for token in ("purpose", "remarks", "explanation", "why", "what", "where", "when")
    )
    return "textarea" if multiline else "text"


def entry_mode(node: SchemaNode) -> str:
    if node.effective_source == "T1":
        return "user_confirmed_input"
    if node.effective_source == "T2":
        return "computed_readonly"
    if node.effective_source == "T3":
        return "model_prefill_review"
    return "structural"


def review_priority(node: SchemaNode) -> str:
    critical = ("amount", "currency", "date", "expense_type", "payee", "airline", "hotel", "rate")
    if node.required or node.required_expression:
        if any(token in node.path_string for token in critical):
            return "high"
        return "medium"
    return "low"


def flatten_nodes(node: SchemaNode) -> list[SchemaNode]:
    nodes = [node]
    if node.node_type == "array" and node.item is not None:
        nodes.extend(flatten_nodes(node.item))
    elif node.node_type == "object":
        for child in node.fields:
            nodes.extend(flatten_nodes(child))
    return nodes


def hydrate_effective_source(node: SchemaNode, inherited_source: str | None = None) -> None:
    node.effective_source = node.source or inherited_source
    if node.node_type == "array" and node.item is not None:
        hydrate_effective_source(node.item, node.effective_source)
    elif node.node_type == "object":
        for child in node.fields:
            hydrate_effective_source(child, node.effective_source)


def build_validation_rules(root: SchemaNode, schema_version: str, generated_at: str) -> dict[str, Any]:
    nodes = flatten_nodes(root)
    field_rules: list[dict[str, Any]] = []
    conditional_rules: list[dict[str, Any]] = []

    for node in nodes:
        field_rules.append(
            {
                "path": node.path_string,
                "node_kind": node_kind(node),
                "schema_type": node.node_type,
                "rust_type": rust_type(node) if node.node_type != "object" else class_name(node),
                "required": node.required and node.required_expression is None,
                "required_expression": node.required_expression,
                "source": node.source,
                "effective_source": node.effective_source,
                "depends_on": node.depends_on or [],
                "allowed_values": node.allowed_values or [],
                "default": node.default,
                "description": node.description,
                "infer_from": node.infer_from,
                "validation": node.validation,
            }
        )

        if node.required_expression:
            conditional_rules.append(
                {
                    "rule_type": "required_when",
                    "target_path": node.path_string,
                    "expression": node.required_expression,
                }
            )
        if node.depends_on:
            conditional_rules.append(
                {
                    "rule_type": "depends_on",
                    "target_path": node.path_string,
                    "depends_on": node.depends_on,
                }
            )
        if node.validation:
            conditional_rules.append(
                {
                    "rule_type": "validation_expression",
                    "target_path": node.path_string,
                    "expression": node.validation,
                }
            )

    return {
        "schema_version": schema_version,
        "generated_at_utc": generated_at,
        "field_rules": field_rules,
        "conditional_rules": conditional_rules,
    }


def rust_string(value: str) -> str:
    rendered = json.dumps(value, ensure_ascii=True)
    return re.sub(r"\\u([0-9a-fA-F]{4})", r"\\u{\1}", rendered)


def rust_optional_string(value: Any) -> str:
    if value is None:
        return "None"
    if isinstance(value, bool):
        rendered = "true" if value else "false"
    else:
        rendered = str(value)
    return f"Some({rust_string(rendered)})"


def rust_string_slice(values: list[str]) -> str:
    if not values:
        return "&[]"
    return "&[" + ", ".join(rust_string(value) for value in values) + "]"


def rust_node_kind(kind: str) -> str:
    mapping = {"object": "Object", "array": "Array", "field": "Field"}
    return f"NodeKind::{mapping[kind]}"


def rust_schema_type(schema_type: str) -> str:
    mapping = {
        "object": "Object",
        "array": "Array",
        "string": "String",
        "number": "Number",
        "date": "Date",
        "enum": "Enum",
        "boolean": "Boolean",
    }
    return f"SchemaType::{mapping[schema_type]}"


def rust_source_tier(source: str | None) -> str:
    if source is None:
        return "None"
    return f"Some(SourceTier::{source})"


def rust_rule_type(rule_type: str) -> str:
    mapping = {
        "required_when": "RequiredWhen",
        "depends_on": "DependsOn",
        "validation_expression": "ValidationExpression",
    }
    return f"ConditionalRuleType::{mapping[rule_type]}"


def generate_rust_validation_rules(validation_rules: dict[str, Any], schema_version: str) -> str:
    lines: list[str] = [
        "// Auto-generated validation rules from schema.yaml. Do not edit manually.",
        "",
        f'pub const VALIDATION_SCHEMA_VERSION: &str = "{schema_version}";',
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "pub enum NodeKind {",
        "    Object,",
        "    Array,",
        "    Field,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "pub enum SchemaType {",
        "    Object,",
        "    Array,",
        "    String,",
        "    Number,",
        "    Date,",
        "    Enum,",
        "    Boolean,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "pub enum SourceTier {",
        "    T1,",
        "    T2,",
        "    T3,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]",
        "pub enum ConditionalRuleType {",
        "    RequiredWhen,",
        "    DependsOn,",
        "    ValidationExpression,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct FieldRule {",
        "    pub path: &'static str,",
        "    pub node_kind: NodeKind,",
        "    pub schema_type: SchemaType,",
        "    pub rust_type: &'static str,",
        "    pub required: bool,",
        "    pub required_expression: Option<&'static str>,",
        "    pub source: Option<SourceTier>,",
        "    pub effective_source: Option<SourceTier>,",
        "    pub depends_on: &'static [&'static str],",
        "    pub allowed_values: &'static [&'static str],",
        "    pub default_value: Option<&'static str>,",
        "    pub description: Option<&'static str>,",
        "    pub infer_from: Option<&'static str>,",
        "    pub validation: Option<&'static str>,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct ConditionalRule {",
        "    pub rule_type: ConditionalRuleType,",
        "    pub target_path: &'static str,",
        "    pub expression: Option<&'static str>,",
        "    pub depends_on: &'static [&'static str],",
        "}",
        "",
        "pub const FIELD_RULES: &[FieldRule] = &[",
    ]

    for rule in validation_rules["field_rules"]:
        lines.append("    FieldRule {")
        lines.append(f"        path: {rust_string(rule['path'])},")
        lines.append(f"        node_kind: {rust_node_kind(rule['node_kind'])},")
        lines.append(f"        schema_type: {rust_schema_type(rule['schema_type'])},")
        lines.append(f"        rust_type: {rust_string(rule['rust_type'])},")
        lines.append(f"        required: {'true' if rule['required'] else 'false'},")
        lines.append(f"        required_expression: {rust_optional_string(rule['required_expression'])},")
        lines.append(f"        source: {rust_source_tier(rule['source'])},")
        lines.append(f"        effective_source: {rust_source_tier(rule['effective_source'])},")
        lines.append(f"        depends_on: {rust_string_slice(rule['depends_on'])},")
        lines.append(f"        allowed_values: {rust_string_slice(rule['allowed_values'])},")
        lines.append(f"        default_value: {rust_optional_string(rule['default'])},")
        lines.append(f"        description: {rust_optional_string(rule['description'])},")
        lines.append(f"        infer_from: {rust_optional_string(rule['infer_from'])},")
        lines.append(f"        validation: {rust_optional_string(rule['validation'])},")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    lines.append("pub const CONDITIONAL_RULES: &[ConditionalRule] = &[")
    for rule in validation_rules["conditional_rules"]:
        depends_on = rule.get("depends_on", [])
        lines.append("    ConditionalRule {")
        lines.append(f"        rule_type: {rust_rule_type(rule['rule_type'])},")
        lines.append(f"        target_path: {rust_string(rule['target_path'])},")
        lines.append(f"        expression: {rust_optional_string(rule.get('expression'))},")
        lines.append(f"        depends_on: {rust_string_slice(depends_on)},")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    lines.append("pub fn field_rule(path: &str) -> Option<&'static FieldRule> {")
    lines.append("    FIELD_RULES.iter().find(|rule| rule.path == path)")
    lines.append("}")
    lines.append("")
    lines.append("pub fn conditional_rules_for(path: &str) -> Vec<&'static ConditionalRule> {")
    lines.append("    CONDITIONAL_RULES.iter().filter(|rule| rule.target_path == path).collect()")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def build_ui_field_map(root: SchemaNode, schema_version: str, generated_at: str) -> dict[str, Any]:
    sections: list[dict[str, Any]] = []
    for section in root.fields:
        repeated = section.node_type == "array"
        fields: list[dict[str, Any]] = []
        for leaf in leaf_nodes(section):
            fields.append(
                {
                    "path": leaf.path_string,
                    "label": titleize(leaf.name),
                    "control": control_type(leaf),
                    "source": leaf.effective_source,
                    "entry_mode": entry_mode(leaf),
                    "required": leaf.required and leaf.required_expression is None,
                    "required_expression": leaf.required_expression,
                    "review_priority": review_priority(leaf),
                    "allowed_values": leaf.allowed_values or [],
                }
            )
        sections.append(
            {
                "key": section.name,
                "label": titleize(section.name),
                "path": section.path_string,
                "repeated": repeated,
                "fields": fields,
            }
        )

    return {
        "schema_version": schema_version,
        "generated_at_utc": generated_at,
        "sections": sections,
    }


def write_text(path: Path, contents: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")


def write_yaml(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        yaml.safe_dump(payload, handle, sort_keys=False, allow_unicode=False, width=120)


def generate(schema_path: Path, output_dir: Path) -> None:
    schema = load_schema(schema_path)
    schema_version = str(schema["schema_version"])
    root = parse_node("expense_report", schema["expense_report"], ("expense_report",))
    hydrate_effective_source(root)
    generated_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()

    model_contents = generate_rust_model(root, schema_version)
    validation_rules = build_validation_rules(root, schema_version, generated_at)
    rust_validation_rules = generate_rust_validation_rules(validation_rules, schema_version)
    ui_field_map = build_ui_field_map(root, schema_version, generated_at)

    for stale_path in (output_dir / "__init__.py", output_dir / "expense_report_model.py"):
        stale_path.unlink(missing_ok=True)
    write_text(output_dir / "expense_report_model.rs", model_contents)
    write_text(output_dir / "validation_rules.rs", rust_validation_rules)
    write_yaml(output_dir / "validation_rules.yaml", validation_rules)
    write_yaml(output_dir / "ui_field_map.yaml", ui_field_map)


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate typed model and rule artifacts from schema.yaml")
    parser.add_argument("--schema", type=Path, default=Path("schema.yaml"))
    parser.add_argument("--output-dir", type=Path, default=Path("generated"))
    args = parser.parse_args()
    generate(args.schema, args.output_dir)


if __name__ == "__main__":
    main()
