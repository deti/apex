---
id: 01KNZ4RPCY9XVZE6M8PZQKJHJA
title: "Exploiting Input Sanitization for Regex Denial of Service (Barlas, Du, Davis, ICSE 2022)"
type: literature
tags: [paper, redos, cwe-1333, icse, 2022, davis, sanitization, web-services, black-box-study]
links:
  - target: 01KNZ301FVXKKT846W2GFQ6QZN
    type: extends
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: extends
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNZ4RPCZDE4HJ02WBKR95DVG
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://davisjam.github.io/files/publications/BarlasDuDavis-WebREDOS-ICSE2022.pdf"
arxiv: "2303.01996"
doi: "10.1145/3510003.3510047"
venue: "ICSE 2022"
authors: [Efe Barlas, Xin Du, James C. Davis]
year: 2022
---

# Exploiting Input Sanitization for Regex Denial of Service

**Authors:** Efe Barlas (Purdue University), Xin Du (Purdue University), James C. Davis (Purdue University).
**Venue:** 44th International Conference on Software Engineering (ICSE '22), Pittsburgh, May 2022.
**DOI:** 10.1145/3510003.3510047. **arXiv mirror:** 2303.01996.
**Author PDF:** https://davisjam.github.io/files/publications/BarlasDuDavis-WebREDOS-ICSE2022.pdf

## Positioning

This is the third major pillar of James C. Davis's multi-year research programme on ReDoS. The first was the ecosystem-scale empirical study at ESEC/FSE 2018 that demonstrated how widespread ReDoS is in open source (see `01KNWEGYBDMW0V0CFVJ83A4B9J`). The second was the S&P 2021 paper that proposed selective memoization as an engine-level defense (see `01KNZ301FVXKKT846W2GFQ6QZN`). This ICSE 2022 paper turns the telescope toward **live commercial web services** and asks a black-box question: *how many of them are exploitable through their publicly-advertised sanitization logic*?

The answer is disturbing: **some major vendors — including Microsoft and Amazon Web Services — published regular expressions that could be weaponised for denial of service against their own APIs, and those regexes were reachable from an unauthenticated network path.** The paper is a direct demonstration that ReDoS is not an academic concern confined to open-source libraries; it is a live vulnerability class in production cloud services in 2022.

## The "consistent sanitization" threat model

The key observation that unlocks the paper's black-box methodology is that **web services sometimes publish their input sanitization regexes to their own clients**. There are two legitimate reasons to do this:

1. **HTML forms** often include `pattern="..."` attributes, which the browser enforces before form submission. The `pattern` value is a JavaScript regex; the same regex is typically used server-side to revalidate the input after submission (so that a non-browser client cannot bypass the check).
2. **API specifications** — OpenAPI, Swagger, JSON Schema, GraphQL — commonly include regex constraints on string fields (`"pattern": "..."`). These are documented so that client libraries can reject obviously-malformed requests before they go over the wire; the server uses the same regex as its last line of defence.

Both cases rely on an implicit **Consistent Sanitization Assumption**: the client-visible regex and the server-side regex match. When they do, an attacker who reads the published regex can analyse it offline for ReDoS vulnerability, craft a pathological input, and send it to the server. If the regex is vulnerable and the server actually uses it, the server spends exponential or polynomial time on a single request — a denial-of-service amplification with near-zero attack cost.

The authors formalise this as a threat model, confirm through hand-analysis on a sample that the Consistent Sanitization Assumption holds in practice, and then measure how often real services unknowingly publish exploitable regexes.

## Methodology

A fully black-box, scale-the-web style study:

1. **Corpus collection.** The authors crawl 1,000 web services with HTML forms and 475 services with publicly-documented APIs. The HTML-form corpus is drawn from the Alexa top list (diverse verticals); the API corpus is drawn from published OpenAPI / Swagger / JSON Schema specifications.
2. **Regex extraction.** For HTML forms, every `pattern` attribute is extracted. For APIs, every `pattern` field in a JSON Schema is extracted.
3. **ReDoS analysis.** Each extracted regex is fed through existing ReDoS detectors (the same detectors used in the Davis group's earlier ecosystem study and in Regexploit; see `01KNZ301FV9DXAXN39MPPAG9JV`) to identify super-linear patterns. Detectors produce both the vulnerability verdict and, where possible, an attack string that triggers the worst case.
4. **Live exploitation probe.** For each vulnerable regex, the authors construct a request containing the attack string and probe the service, measuring response time against a baseline. A ReDoS vulnerability is confirmed if the response time grows super-linearly with the attack string length, matching the local prediction from the detector.
5. **Responsible disclosure.** All confirmed vulnerabilities are reported to the affected vendors before publication.

## Findings

- **1,000 HTML-form services:** 355 publish at least one regex in their forms. On inspection, 17 publish **unsafe** regexes (detector verdict: super-linear). Of those, the authors were able to exploit **6 services** via network probes — i.e., the live service actually runs the published regex against unsanitized input with sufficient time budget for an attacker to observe a measurable slowdown. The 6 services cover 6 domains and 15 subdomains (some services host multiple vulnerable endpoints).
- **475 API services:** similar extraction over OpenAPI / JSON Schema yields analogous findings; the paper reports the same 6 confirmed cases were reached via the API path rather than the HTML form, indicating that **the vulnerability class reaches modern REST and GraphQL APIs, not just legacy forms**.
- **Notable disclosures:** both Microsoft and Amazon Web Services were among the affected vendors and patched their public services in response to the disclosure.

## Mitigation the authors shipped

Beyond reporting the individual CVEs, the authors contributed a **ReDoS defense patch to a popular API validation library** (exact library named in the paper and its artifact). The patch adds regex vulnerability detection to the validator's startup path so that when a service loads its OpenAPI schema, unsafe `pattern` constraints trigger a warning or refusal before the service starts accepting traffic. The patch was merged upstream.

## Why this paper matters for APEX G-46

1. **Regex ReDoS is a live cloud vulnerability class.** Any APEX detector that flags a vulnerable regex on attacker input is flagging a real class of bug; this paper is the empirical grounding that justifies G-46 prioritising ReDoS above abstract computational attacks.
2. **The Consistent Sanitization Assumption gives APEX a new scan target.** If APEX can analyse an OpenAPI / JSON Schema document for ReDoS on its `pattern` fields, it can produce *pre-deployment* findings about a service's exposed sanitizers without needing source code access at all. This is a natural scan mode for a SaaS-oriented deployment of APEX.
3. **The 6-out-of-1000 hit rate is a calibration point.** A G-46 benchmark built on top of the Barlas/Du/Davis corpus could measure how many of the 1000 services an APEX scan could pre-classify correctly as vulnerable or safe, given only the published pattern attributes.
4. **The patched validator is a citable remediation.** When APEX flags a vulnerable pattern in an OpenAPI spec, the report should recommend upgrading the validator to the version including the Barlas/Du/Davis patch.

## Citation

```
@inproceedings{barlas2022exploiting,
  author    = {Efe Barlas and Xin Du and James C. Davis},
  title     = {Exploiting Input Sanitization for Regex Denial of Service},
  booktitle = {Proceedings of the 44th International Conference on Software Engineering (ICSE '22)},
  year      = {2022},
  pages     = {883--895},
  publisher = {ACM},
  doi       = {10.1145/3510003.3510047}
}
```

## References

- Author PDF — [davisjam.github.io/files/publications/BarlasDuDavis-WebREDOS-ICSE2022.pdf](https://davisjam.github.io/files/publications/BarlasDuDavis-WebREDOS-ICSE2022.pdf)
- arXiv — [arxiv.org/abs/2303.01996](https://arxiv.org/abs/2303.01996)
- ICSE conference page — [conf.researchr.org/details/icse-2022/icse-2022-papers/11/Exploiting-Input-Sanitization-for-Regex-Denial-of-Service](https://conf.researchr.org/details/icse-2022/icse-2022-papers/11/Exploiting-Input-Sanitization-for-Regex-Denial-of-Service)
- Purdue ECE pubs — [docs.lib.purdue.edu/ecepubs/162](https://docs.lib.purdue.edu/ecepubs/162/)
- Davis group memoization paper (S&P 2021) — see `01KNZ301FVXKKT846W2GFQ6QZN`
- Davis ecosystem study (ESEC/FSE 2018) — see `01KNWEGYBDMW0V0CFVJ83A4B9J`
- Regexploit — see `01KNZ301FV9DXAXN39MPPAG9JV`
