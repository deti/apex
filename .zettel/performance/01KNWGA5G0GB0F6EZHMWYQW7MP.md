---
id: 01KNWGA5G0GB0F6EZHMWYQW7MP
title: "Wikipedia: Billion Laughs Attack"
type: literature
tags: [wikipedia, billion-laughs, xml, yaml, dos, resource-exhaustion]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5FRK4HKHP4ZX35ZZ9FB
    type: references
  - target: 01KNWGA5GJ1VC19DXXAX3SS947
    type: references
  - target: 01KNZ2ZDMJNCKQ2AZEYXENBX53
    type: related
created: 2026-04-10
modified: 2026-04-12
source: "https://en.wikipedia.org/wiki/Billion_laughs_attack"
---

# Billion Laughs Attack (Wikipedia)

*Source: https://en.wikipedia.org/wiki/Billion_laughs_attack — fetched 2026-04-10.*

## Overview

A **billion laughs attack** is a "type of denial-of-service attack (DoS attack) which is aimed at parsers of XML documents." Alternative names include **XML bomb** or **exponential entity expansion attack**.

## Details

The attack mechanism involves defining multiple entities where each successive entity references the previous one repeatedly. Wikipedia explains: "The example attack consists of defining 10 entities, each defined as consisting of 10 of the previous entity, with the document consisting of a single instance of the largest entity, which expands to one billion copies of the first entity."

The naming derives from the base entity, typically the string "lol". When parsed, "a billion instances of the string 'lol' would likely exceed [the memory] available to the process parsing the XML."

**History**: The problem emerged around 2002 but "began to be widely addressed in 2008."

**Defences**: Wikipedia mentions "capping the memory allocated in an individual parser if loss of the document is acceptable, or treating entities symbolically and expanding them lazily."

## Canonical Example

The article provides a complete XML demonstration showing nested entity definitions (`lol` through `lol9`), resulting in approximately 3 gigabytes of memory consumption from a sub-1KB file:

```xml
<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY lol "lol">
  <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
  <!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
  <!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
  <!ENTITY lol6 "&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;">
  <!ENTITY lol7 "&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;">
  <!ENTITY lol8 "&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;">
  <!ENTITY lol9 "&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;">
]>
<lolz>&lol9;</lolz>
```

## Variations

The **quadratic blowup** variant uses "quadratic growth in resource requirements by simply repeating a large entity over and over again." It avoids the recursive-definition signature that some parsers detect, instead achieving comparable damage with a long flat expansion.

The article also documents **YAML-based attacks** affecting systems like Kubernetes. Go's YAML processor was "modified to fail parsing if the result object becomes too large" in response.

## Relevance to APEX G-46

This is the textbook amplification attack that any parser handling untrusted input must defend against, yet remains surprisingly common in production. APEX's ReDoS cousin — the billion-laughs detector — should be a first-class capability in G-46. Detection is straightforward (static pattern: XML with entity declarations whose right-hand-side references other entities; YAML with anchor/alias references); verification is equally cheap (instantiate the parser on the witness and watch memory growth).

The existence of YAML anchor bombs is a reminder that APEX's detector should cover *all* template/reference-expansion formats, not just XML: YAML, LDIF, Jinja/Mako, XSLT, even protobuf text-format.
