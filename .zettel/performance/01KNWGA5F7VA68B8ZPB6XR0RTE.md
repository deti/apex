---
id: 01KNWGA5F7VA68B8ZPB6XR0RTE
title: "CWE-1333: Inefficient Regular Expression Complexity"
type: literature
tags: [cwe, cwe-1333, mitre, redos, regex, security, backtracking]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FPC1MSYBQQS6GJPRTS
    type: references
  - target: 01KNWGA5G3XDK746J4N59G6VVW
    type: references
  - target: 01KNWGA5G5JNAQP0QEYZXN6T2H
    type: references
  - target: 01KNZ301FVXKKT846W2GFQ6QZN
    type: references
  - target: 01KNZ301FVQZCT0JNP97SDY1MH
    type: references
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: references
  - target: 01KNZ301FVEJEFXWZQRNCB36SS
    type: references
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: references
  - target: 01KNZ301FVAC85VSD6QSXHBTBN
    type: references
  - target: 01KNZ301FVRCH0P2GZS4CNGJ23
    type: references
  - target: 01KNZ301FVV2BBBW67QZV0MWTM
    type: references
  - target: 01KNZ2ZDM5DNHY26MRQ9V2BPKT
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://cwe.mitre.org/data/definitions/1333.html"
---

# CWE-1333: Inefficient Regular Expression Complexity

*Source: https://cwe.mitre.org/data/definitions/1333.html — fetched 2026-04-10.*

## Description

"The product uses a regular expression with an inefficient, possibly exponential worst-case computational complexity that consumes excessive CPU cycles."

## Extended Description

"Some regular expression engines have a feature called 'backtracking'. If the token cannot match, the engine 'backtracks' to a position that may result in a different token that can match. Backtracking becomes a weakness if all of these conditions are met:

- The number of possible backtracking attempts are exponential relative to the length of the input.
- The input can fail to match the regular expression.
- The input can be long enough.

Attackers can create crafted inputs that intentionally cause the regular expression to use excessive backtracking in a way that causes the CPU consumption to spike."

## Common Consequences

**Impact: DoS: Resource Consumption (CPU)**
- Scope: Availability
- Likelihood: High

## Potential Mitigations

**Architecture and Design:**
"Use regular expressions that do not support backtracking, e.g. by removing nested quantifiers." (Effectiveness: High)

Note: "This is one of the few effective solutions when using user-provided regular expressions."

**System Configuration:**
"Set backtracking limits in the configuration of the regular expression implementation, such as PHP's pcre.backtrack_limit. Also consider limits on execution time for the process." (Effectiveness: Moderate)

**Implementation:**
- "Do not use regular expressions with untrusted input. If regular expressions must be used, avoid using backtracking in the expression." (Effectiveness: High)
- "Limit the length of the input that the regular expression will process." (Effectiveness: Moderate)

## Relationships

**ChildOf:** CWE-407 (Inefficient Algorithmic Complexity)

**MemberOf:** CWE-1226 (Complexity Issues)

## Demonstrative Examples

**Example 1 (JavaScript — Bad Code):**
```javascript
var test_string = "Bad characters: $@#";
var bad_pattern = /^(\w+\s?)*$/i;
var result = test_string.search(bad_pattern);
```

"The regular expression has a vulnerable backtracking clause inside `(\w+\s?)*$` which can be triggered to cause a Denial of Service by processing particular phrases."

**Example 1 (JavaScript — Good Code):**
```javascript
var test_string = "Bad characters: $@#";
var good_pattern = /^((?=(\w+))\2\s?)*$/i;
var result = test_string.search(good_pattern);
```

**Example 2 (Perl — Bad Code):**
```perl
my $test_string = "Bad characters: \$\@\#";
my $bdrslt = $test_string;
$bdrslt =~ /^(\w+\s?)*$/i;
```

**Example 2 (Perl — Good Code):**
```perl
my $test_string = "Bad characters: \$\@\#";
my $gdrslt = $test_string;
$gdrslt =~ /^((?=(\w+))\2\s?)*$/i;
```

## Observed Examples

- **CVE-2020-5243** — Server allows ReDoS with crafted User-Agent strings due to overlapping capture groups causing excessive backtracking
- **CVE-2021-21317** — npm package for user-agent parser prone to ReDoS due to overlapping capture groups
- **CVE-2019-16215** — Markdown parser uses inefficient regex when processing messages, allowing users to cause CPU consumption
- **CVE-2019-6785** — Long string in version control product allows DoS due to inefficient regex
- **CVE-2019-12041** — JavaScript code allows ReDoS via long string due to excessive backtracking
- **CVE-2015-8315** — ReDoS when parsing time
- **CVE-2015-8854** — ReDoS when parsing documents
- **CVE-2017-16021** — ReDoS when validating URL

## References (from the CWE page)

- Scott A. Crosby. "Regular Expression Denial of Service". 2003-08
- Jan Goyvaerts. "Runaway Regular Expressions: Catastrophic Backtracking". 2019-12-22
- Adar Weidman. "Regular expression Denial of Service - ReDoS"
- Ilya Kantor. "Catastrophic backtracking". 2020-12-13
- Cristian-Alexandru Staicu and Michael Pradel. "Freezing the Web: A Study of ReDoS Vulnerabilities in JavaScript-based Web Servers". 2018-07-11
- James C. Davis et al. "The Impact of Regular Expression Denial of Service (ReDoS) in Practice: An Empirical Study at the Ecosystem Scale". 2018-08-01
- James Davis. "The Regular Expression Denial of Service (ReDoS) cheat-sheet". 2020-05-23

## Relevance to APEX G-46

This is the specific CWE that APEX ReDoS findings should carry. The G-46 acceptance criteria include ≥80% ReDoS recall on a benchmark corpus and a requirement that each ReDoS finding include a concrete worst-case input string — which aligns directly with the "input can be long enough" third clause in the extended description above.
