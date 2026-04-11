---
id: 01KNWGA5GJ1VC19DXXAX3SS947
title: "Tool: defusedxml (Python XML Hardening)"
type: literature
tags: [tool, defusedxml, python, xml, billion-laughs, security]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://github.com/tiran/defusedxml"
---

# defusedxml — XML Bomb Protection for Python

*Source: https://github.com/tiran/defusedxml — fetched 2026-04-10.*

## Primary XML Attack Vectors Defended

The defusedxml package protects against several critical XML vulnerabilities:

**Billion Laughs Attack** — Uses nested entities to expand a small XML document into gigabytes of memory within seconds.

**Quadratic Blowup** — Repeats large entities throughout a document rather than nesting them, avoiding some parser countermeasures while still consuming excessive resources.

**External Entity Expansion (Remote)** — Leverages entity declarations pointing to external URLs, potentially enabling attackers to circumvent firewalls, attack third-party services, or conduct reconnaissance.

**External Entity Expansion (Local File)** — References local files via `file://` URLs or relative paths, potentially exposing sensitive configuration data.

**DTD Retrieval** — Allows parsers to fetch remote document type definitions, opening similar attack surfaces as external entities.

## Core API Functions

The library provides defused versions of standard Python XML modules:

- **defusedxml.ElementTree** — `parse()`, `iterparse()`, `fromstring()`, `XMLParser`
- **defusedxml.sax** — `parse()`, `parseString()`, `make_parser()`
- **defusedxml.minidom** — `parse()`, `parseString()`
- **defusedxml.pulldom** — `parse()`, `parseString()`
- **defusedxml.expatbuilder** — `parse()`, `parseString()`
- **defusedxml.xmlrpc** — monkey-patch module for securing XML-RPC

## Key Configuration Parameters

Functions accept three protective keyword arguments:

- `forbid_dtd` (default: `False`) — blocks DOCTYPE declarations
- `forbid_entities` (default: `True`) — prevents entity declarations
- `forbid_external` (default: `True`) — disables external resource access

## Relevance to APEX G-46

defusedxml is the defence-side counterpart to the billion-laughs attack. APEX's Python detector should flag any use of the standard `xml.etree`, `xml.sax`, `xml.minidom`, `xml.pulldom`, or `xml.expatbuilder` modules on user-controlled input, and recommend defusedxml as the mitigation. The fact that Python's stdlib XML parsers still default to accepting external entities and unbounded entity expansion (unless the caller explicitly opts into defusedxml) means this is a high-precision, actionable detector with almost no false positives.
