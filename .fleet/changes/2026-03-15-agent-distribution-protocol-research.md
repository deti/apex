# Deep Research: Open Agent Distribution Protocol Landscape

**Date:** 2026-03-15
**Scope:** Adjacent standards, community discussions, and design patterns relevant to an open agent distribution protocol

---

## Executive Summary

The agent ecosystem as of March 2026 has converged around **three governance pillars** (AAIF, AGNTCY, NIST) and **five protocol layers** (MCP for tool access, A2A for agent-to-agent communication, AGENTS.md for repo guidance, Agent Skills for capability packaging, SLIM for network messaging). Despite this rapid standardization, a critical gap persists: **there is no open standard for agent distribution as a first-class artifact**. Distribution today is fragmented across proprietary registries (Smithery, Glama), framework-specific mechanisms, and ad-hoc directory listings. The analogy is pre-OCI containers---everyone can build agents, but packaging, signing, versioning, and distributing them portably remains unsolved.

---

## 1. The MCP Ecosystem

### Current State
- **MCP Registry** launched September 2025 as an open catalog and API for MCP servers. Entered API freeze (v0.1) in October 2025.
- **Architecture**: Central authoritative registry with public "subregistries" (opinionated marketplaces per client) and private enterprise subregistries.
- **Discovery**: `.well-known/mcp.json` endpoint (SEP-1960) allows server metadata discovery without live connections.
- **Governance**: Donated to AAIF (Linux Foundation) in December 2025. Now adopted by Anthropic, OpenAI, Google, Microsoft, AWS.

### 2026 Roadmap
- Transport scalability (Streamable HTTP horizontal scaling without state)
- Agent-to-agent extensions: MCP servers can act as agents themselves ("fractal" agentic systems)
- Governance maturation: contributor ladder, working groups, SEP delegation model
- Enterprise readiness: audit trails, SSO, gateway behavior, configuration portability
- **No new transports**---evolution of existing ones only
- Elicitation (servers request structured input from users) and Sampling (servers request LLM completions from clients) are now specified

### What MCP Does NOT Cover
- **Agent packaging/bundling**: No standard format for distributing an entire agent
- **Agent versioning**: No semver contract for agent behavior
- **Agent composition**: No way to declare that Agent A depends on Agent B
- **Agent testing/verification**: No conformance suite for agent behavior

### Key Takeaway
MCP solves **agent-to-tool connectivity**. It does not solve agent distribution, agent-to-agent coordination (that's A2A), or agent packaging.

---

## 2. Agent2Agent Protocol (A2A)

### Design
- Launched by Google April 2025. Donated to Linux Foundation June 2025. 100+ supporting companies.
- **Agent Cards**: JSON documents at `/.well-known/agent-card.json` describing identity, capabilities, skills, communication methods, security requirements.
- **Task Management**: Stateful task lifecycle---creation, execution, retry, expiry.
- **Capability Discovery**: Agents advertise what they can do; clients discover the best agent for a task.
- Release Candidate v1.0 reached January 2026.

### Identified Gaps
- No machine-readable definitions of skill inputs/outputs (no JSON Schema or typed fields), making fully automated orchestration harder.
- Authentication supported but **no standard authorization model** prescribed---"authorization creep" risk in multi-agent systems.
- No agent packaging or distribution mechanism---Agent Cards describe *running* agents, not distributable artifacts.

### Key Takeaway
A2A solves **agent-to-agent communication** and **capability advertisement** for live services. It does not solve packaging, offline distribution, or portable execution.

---

## 3. AGENTS.md

### Design
- Released by OpenAI August 2025. Adopted by 60,000+ open-source projects.
- Markdown-based convention providing project-specific guidance for coding agents.
- Supported by GitHub Copilot, VS Code, Cursor, Codex, Devin, Gemini CLI, Jules, Amp, Windsurf, Zed, RooCode.
- Donated to AAIF December 2025.

### What It Covers
- Repository-level agent guidance: coding conventions, build steps, file structure rules.
- Functions as an agent's "README for a repo."

### What It Does NOT Cover
- Not a distribution format. Not a packaging standard. Not a capability manifest.
- Scoped to coding agents operating within repositories.

---

## 4. Anthropic Agent Skills

### Design
- Released December 2025. Published as open standard at agentskills.io.
- **Directory-based format**: Folder containing `SKILL.md` + supporting files (scripts, data, resources).
- **SKILL.md**: YAML frontmatter (name, description) + body instructions.
- **Progressive disclosure**: Three levels---metadata at startup, full SKILL.md on trigger, supporting files on demand.
- Adopted by VS Code, GitHub, Cursor, Goose, Amp, OpenCode.
- Skills directory launched with partners: Atlassian, Canva, Cloudflare, Figma, Notion, Ramp, Sentry.

### Packaging Model
- Standard filesystem organization---no proprietary archive format.
- Skills bundle executable scripts (Python, Bash) that agents invoke via code execution tools.
- Designed for **reusable procedural knowledge** packaged into shareable modules.

### Key Takeaway
Agent Skills is the **closest thing to an agent distribution format** that exists today. However:
- It packages *knowledge/instructions*, not *executable agent runtimes*.
- No dependency management, no versioning contract, no signing/verification.
- No registry protocol (directory is curated, not a programmatic registry).
- Filesystem-based---no OCI-style content-addressable distribution.

---

## 5. AGNTCY Project

### Design
- Cisco-originated, donated to Linux Foundation July 2025. 65+ supporting companies.
- Four pillars: Agent Discovery, Agent Identity, Agent Messaging (SLIM protocol), Agent Observability.
- **SLIM** (Secure Low-latency Interactive Messaging): Network-level agent communication protocol with quantum-safe communications support.
- **Open Agent Schema Framework**: Agent discovery and description.
- Interoperable with A2A and MCP.

### Key Takeaway
AGNTCY provides **infrastructure plumbing** (identity, messaging, observability) but not a distribution format.

---

## 6. Governance Bodies

### Agentic AI Foundation (AAIF)
- Launched December 2025 under Linux Foundation.
- Co-founded by OpenAI, Anthropic, Block.
- Platinum members: AWS, Bloomberg, Cloudflare, Google, Microsoft.
- Gold members: Cisco, Datadog, Docker, IBM, JetBrains, Okta, Oracle, Salesforce, SAP, Shopify, Snowflake, Temporal, Twilio.
- Stewards MCP, Goose, AGENTS.md.
- MCP Dev Summit North America 2026: April 2-3, NYC, 95+ sessions.

### NIST AI Agent Standards Initiative
- Announced February 2026 by Center for AI Standards and Innovation (CAISI).
- Three pillars: industry-led standards, community-led open source protocols, AI agent security/identity research.
- RFI on Agent Security due March 9, 2026. Agent Identity concept paper due April 2.
- Sector-specific listening sessions starting April 2026.

### Key Observation
Governance is consolidating rapidly. The AAIF has critical mass. NIST provides the regulatory dimension. But **neither has addressed agent distribution as a distinct concern**.

---

## 7. OCI Analogy: Lessons for Agent Packaging

### How OCI Succeeded
- Established June 2015 when Docker donated runtime/image format to neutral governance.
- **Three specs**: Runtime (how to run), Image (how to package), Distribution (how to move).
- **Design principles**: Minimalism, backward compatibility, vendor neutrality.
- **Key insight**: Docker was already the de facto standard. OCI formalized it rather than inventing from scratch.
- OCI v1.1 (2024) added generic artifact support: Helm charts, WASM modules, SBOMs, policy bundles, model weights can all be stored as OCI artifacts.

### Applicability to Agents
- **OCI artifacts already support arbitrary content types**. Agent bundles could be distributed as OCI artifacts today.
- The `artifactType` and `subject` fields in OCI 1.1 allow associating metadata (signatures, SBOMs) with agent artifacts.
- OCI registries (Docker Hub, GitHub Container Registry, AWS ECR, Azure ACR) already handle auth, access control, mirroring.
- **Docker has explicitly adopted OCI artifacts for AI model packaging** (announced 2025).

### What's Missing for Agents
- No **agent-specific media type** convention.
- No **agent manifest schema** (equivalent of container image config).
- No **agent runtime spec** (equivalent of OCI runtime spec).

---

## 8. WASM Component Model: Capability-Based Agent Execution

### Current State
- WASI Preview 2 (2024): Sockets, HTTP, CLI, Component Composition.
- WASI Preview 3 (expected 2026): Async I/O and Streams.
- Component Model: Language-agnostic, capability-based composition of modules.

### Relevance to Agent Distribution
- **Wassette** (Microsoft, August 2025): WebAssembly runtime for AI agent tools. Agents autonomously fetch WASM components from OCI registries and execute them. Capability-based security.
- **Capability-based security**: Agents receive explicit, unforgeable tokens for each resource they can access. Every tool invocation becomes an explicit capability grant.
- **Portable execution**: Same artifact runs in browser, edge, cloud without modification.
- NVIDIA published guidance on sandboxing agentic AI workflows with WebAssembly.

### Key Insight
WASM + OCI provides a **proven stack for distributing and executing portable, sandboxed capabilities**. The Component Model's WIT (WebAssembly Interface Types) could serve as a typed capability contract for agent tools.

---

## 9. Existing Registries and Marketplaces

### Smithery.ai
- MCP server registry and hosting platform. Client-agnostic.
- **Hosted mode**: Smithery runs MCP servers on their infrastructure.
- Provides generated OAuth modals, spec compatibility tracking.
- Registry API for programmatic search.

### Glama.ai
- MCP hosting platform. Largest collection of MCP servers.
- Indexes, scans, ranks servers by security, compatibility, ease of use.
- API gateway, ChatGPT-like UI, multiple transports.
- Usage-based sorting (30-day popularity).

### MCP Registry (Official)
- Community-driven. Open catalog and API. API freeze v0.1.
- Supports public subregistries and private enterprise subregistries.

### Microsoft Marketplace
- Unified AppSource + Azure Marketplace. AI apps and agents alongside cloud solutions.
- Broader agent availability expected later 2026.

### Key Observation
All existing registries are **MCP-server registries**, not agent registries. They distribute tool servers, not portable agents.

---

## 10. Package Manager Lessons (npm, crates.io, PyPI)

### Trust and Verification
- **Trusted Publishing** is now the de facto standard across PyPI (2023), RubyGems (2023), crates.io (2025), npm (2025).
- Uses OIDC identity tokens from CI systems---no long-lived secrets.
- 39,000+ PyPI projects adopted trusted publishing.
- **Lesson**: Provenance and supply chain security must be built in from day one, not bolted on.

### Distribution Patterns
- Content-addressable storage (npm tarballs, crate archives).
- Semantic versioning as a behavioral contract.
- Dependency resolution with lockfiles.
- Namespace/scope conventions for organizational ownership.
- Automated publishing from CI/CD pipelines.

### Applicability to Agents
An agent registry would need:
- Content-addressable agent artifacts (hashes, signatures).
- Semantic versioning for agent capabilities.
- Dependency declarations (this agent requires these MCP servers, these skills).
- Trusted publishing from CI (OIDC-based, no API keys).
- Namespace conventions (org/agent-name).

---

## 11. Terraform Registry Patterns

### Relevant Design Patterns
- **Module Registry Protocol**: Standardized API for discovering and downloading modules.
- **Namespace convention**: `terraform-<PROVIDER>-<NAME>` enforces discoverability.
- **Monorepo-to-submodule automation**: Automated workflows sync changes from monorepo to individual repos for registry consumption.
- **Standard structure**: Required directory layout enables automated inspection and documentation generation.

### Applicability to Agents
- A standard agent directory structure could enable automated inspection, testing, and documentation.
- Monorepo-compatible distribution (GitHub Actions sync to registry) is a proven pattern.
- Module versioning and compatibility constraints map well to agent capability versioning.

---

## 12. OpenAPI/AsyncAPI Governance Lessons

### Key Lessons
- **OpenAPI**: Started as Swagger (2011), donated to Linux Foundation (2014). Took 6 years to reach broad adoption as OpenAPI 3.0 (2017).
- **AsyncAPI**: Studied governance models of NodeJS, GraphQL, CNCF, OpenAPI before designing their own.
- **AsyncAPI's principle**: Power goes to people who "work", not companies that "pay." Equal weight for individual and corporate contributors.
- **Both succeeded by**: (1) formalizing an existing de facto standard rather than inventing one, (2) providing immediate tooling value (code generation, documentation), (3) neutral governance under Linux Foundation.

### Pattern for Agent Distribution
- **Formalize what exists** (Agent Skills directory format, A2A Agent Cards, MCP server manifests) rather than inventing from scratch.
- **Governance from day one** under a neutral body (AAIF is the obvious home).
- **Tooling-first**: A standard succeeds when it enables tools people want (agent installers, registries, IDEs, CI pipelines).

---

## 13. Agent Testing and Verification

### Current State
- 52% of organizations run offline evaluations on test sets. 37% run online evals.
- Non-deterministic behavior makes testing fundamentally different from traditional software.
- Key challenge: Agents follow different reasoning paths to correct answers.

### Emerging Approaches
- **HTTP endpoint testing**: Framework-agnostic---test any agent through REST API regardless of underlying framework.
- **Span-level tracing**: Nested execution graphs with session replay for debugging.
- **CI pipeline integration**: Automated multi-agent workflow verification.
- IEEE and Partnership on AI developing evaluation standards.

### Gap
No standard **conformance test suite** for agent distribution. Compare: OCI has `runc` conformance tests. OpenAPI has validators. Agent Skills has nothing analogous.

---

## 14. Capability-Based Security for Agents

### Consensus Position
- **Least-privilege by default**: Agents get minimum, time-bound, verifiable permissions per action.
- **Deterministic enforcement**: At least one enforcement layer must NOT rely on LLM reasoning.
- **WASM as enforcement layer**: Capability-based access at OS boundary. Every tool call wrapped in a WASM module with explicit capability grants.
- **Gartner**: AI Security Platforms identified as top strategic technology trend for 2026.

### NIST Direction
- Agent Security RFI (due March 2026): privilege escalation, misuse, unintended autonomous actions.
- Agent Identity concept paper (due April 2026): cryptographic identity, authorization.

### Implication for Distribution
An agent distribution format must include a **capability manifest**: what permissions does this agent require? This is analogous to Android's `AndroidManifest.xml` or WASM's capability imports.

---

## 15. Community Sentiment and Criticism

### The Protocol Proliferation Problem
InfoWorld: "Producing 20 standards for the same need essentially results in no standards." The article draws parallels to CORBA, DCOM, and WS-* failures.

Competing protocols include:
- A2A (Google/Linux Foundation)
- MCP (Anthropic/AAIF)
- SLIM (AGNTCY)
- ACP (IBM, March 2025)
- LCAP (LangChain)
- Various framework-specific protocols

The critique: "99% of enterprise agent interaction can be handled with a handful of message types: request, response, notify, error." Most churn is "more about gaining mindshare and securing business development budgets than solving architecture issues."

### Framework Lock-in Concerns
- LangGraph: Tightly coupled to LangChain ecosystem, no native protocol support.
- CrewAI: Added A2A support, better interop story.
- AutoGen: Conversational multi-agent focus, limited portability.
- OpenAgents: Claims first-class support for both A2A and MCP.

### What People Are Asking For
From HN, Reddit, and community discussions:
1. "I just want to run someone else's agent locally without rebuilding it"
2. "Agent Skills are instructions, not executables---where's the runtime?"
3. "Why can't I `npm install` an agent?"
4. "Agent Cards describe running services, not distributable packages"
5. "MCP servers are tools, not agents---the naming is confusing"

---

## 16. The Gap Map: What Exists vs. What's Missing

| Concern | Standard/Solution | Status |
|---------|------------------|--------|
| Agent-to-tool connectivity | MCP | Mature, AAIF governance |
| Agent-to-agent communication | A2A | RC v1.0, Linux Foundation |
| Repo-level agent guidance | AGENTS.md | 60K+ repos, AAIF governance |
| Skill/knowledge packaging | Agent Skills | Open standard, early adoption |
| Agent identity | AGNTCY | Infrastructure-level |
| Agent network messaging | SLIM | Specified, early adoption |
| Agent discovery (live) | A2A Agent Cards + .well-known | Specified |
| MCP server discovery | MCP Registry + .well-known/mcp | Operational |
| Agent security standards | NIST initiative | RFI phase |
| **Agent packaging format** | **NONE** | **Gap** |
| **Agent versioning contract** | **NONE** | **Gap** |
| **Agent dependency management** | **NONE** | **Gap** |
| **Agent distribution registry protocol** | **NONE** | **Gap** |
| **Agent conformance testing** | **NONE** | **Gap** |
| **Agent runtime spec** | **NONE (WASM is closest)** | **Gap** |
| **Portable agent execution** | **WASM Component Model (partial)** | **Emerging** |
| **Agent capability manifest** | **NONE (WASM capabilities closest)** | **Gap** |

---

## 17. Synthesis: Design Patterns for an Open Agent Distribution Protocol

Based on this research, a viable agent distribution protocol would draw from:

### From OCI
- **Content-addressable artifacts** with cryptographic digests
- **Manifest + config + layers** structure
- **Distribution spec** (push/pull/referrers API)
- **Media type conventions** for agent-specific content
- Use OCI registries directly (don't reinvent infrastructure)

### From WASM Component Model
- **Capability-based security**: Explicit capability imports/exports
- **WIT interfaces**: Typed contracts for agent interactions
- **Sandboxed execution**: Portable, isolated runtime
- **Component composition**: Agents composed from smaller components

### From Agent Skills
- **Directory-based packaging**: SKILL.md pattern is natural
- **Progressive disclosure**: Metadata first, full content on demand
- **Instructions + scripts + resources** bundling

### From A2A
- **Agent Card** pattern for capability advertisement
- **`.well-known` discovery** mechanism
- **Task lifecycle** model

### From npm/crates.io/PyPI
- **Semantic versioning** as behavioral contract
- **Trusted publishing** via OIDC
- **Dependency resolution** with lockfiles
- **Namespace/scope** conventions

### From Terraform Registry
- **Standard directory structure** enabling automated inspection
- **Module Registry Protocol** for programmatic access
- **Monorepo-compatible** distribution

### From Android/WASM Security
- **Capability manifest**: Declare required permissions upfront
- **Deterministic enforcement**: Non-LLM permission boundary
- **Granular, time-bound** access grants

---

## 18. Strategic Assessment

### Why This Gap Matters Now
1. **AAIF has critical mass** but no distribution standard. The foundation exists; the artifact format doesn't.
2. **NIST is asking** for standards proposals (RFIs due March-April 2026). There is a window.
3. **Agent Skills proved the model** (directory-based packaging works) but lacks the runtime/distribution layer.
4. **OCI 1.1 artifacts** provide ready infrastructure---no need to build registries from scratch.
5. **WASM Component Model** provides the execution sandbox---no need to invent a runtime.

### The Opportunity
Combine Agent Skills packaging model + OCI distribution + WASM execution + A2A discovery into a coherent **Agent Distribution Spec** that answers:
- How do I package an agent? (manifest + skills + capabilities + dependencies)
- How do I distribute it? (OCI registry, content-addressable)
- How do I discover it? (.well-known, registry API)
- How do I verify it? (signatures, trusted publishing, capability audit)
- How do I run it? (WASM sandbox with capability grants)
- How do I test it? (conformance suite, behavioral contracts)

### Risk: Protocol Fatigue
The InfoWorld critique is valid. Another standard risks "20 standards for the same need." The path forward is to **compose existing standards** rather than invent a new one, and to anchor in AAIF governance from day one.

---

## Sources

### MCP Ecosystem
- [Introducing the MCP Registry](http://blog.modelcontextprotocol.io/posts/2025-09-08-mcp-registry-preview/)
- [The 2026 MCP Roadmap](http://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/)
- [MCP Specification 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [Official MCP Registry](https://registry.modelcontextprotocol.io/)
- [MCP Registry GitHub](https://github.com/modelcontextprotocol/registry)
- [SEP: .well-known/mcp Discovery Endpoint](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/1960)

### A2A Protocol
- [A2A Protocol Specification](https://a2a-protocol.org/latest/specification/)
- [Announcing A2A - Google Developers Blog](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/)
- [A2A Protocol Upgrade - Google Cloud Blog](https://cloud.google.com/blog/products/ai-machine-learning/agent2agent-protocol-is-getting-an-upgrade)
- [What Is A2A Protocol - IBM](https://www.ibm.com/think/topics/agent2agent-protocol)

### AAIF and Governance
- [Linux Foundation AAIF Announcement](https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation)
- [Anthropic Donating MCP to AAIF](https://www.anthropic.com/news/donating-the-model-context-protocol-and-establishing-of-the-agentic-ai-foundation)
- [OpenAI Co-founds AAIF](https://openai.com/index/agentic-ai-foundation/)
- [AAIF Guide - IntuitionLabs](https://intuitionlabs.ai/articles/agentic-ai-foundation-open-standards)
- [MCP Dev Summit 2026](https://events.linuxfoundation.org/2026/02/24/agentic-ai-foundation-unveils-mcp-dev-summit-north-america-2026-schedule/)

### Agent Skills
- [Equipping Agents with Agent Skills - Anthropic](https://claude.com/blog/equipping-agents-for-the-real-world-with-agent-skills)
- [Agent Skills Open Standard - The New Stack](https://thenewstack.io/agent-skills-anthropics-next-bid-to-define-ai-standards/)
- [Agent Skills - SiliconANGLE](https://siliconangle.com/2025/12/18/anthropic-makes-agent-skills-open-standard/)
- [Packaging Expertise - O'Reilly](https://www.oreilly.com/radar/packaging-expertise-how-claude-skills-turn-judgment-into-artifacts/)

### AGENTS.md
- [AGENTS.md Specification](https://agents.md/)
- [Complete Guide to AGENTS.md](https://www.remio.ai/post/what-is-agents-md-a-complete-guide-to-the-new-ai-coding-agent-standard-in-2025)

### AGNTCY
- [Linux Foundation AGNTCY Announcement](https://www.linuxfoundation.org/press/linux-foundation-welcomes-the-agntcy-project-to-standardize-open-multi-agent-system-infrastructure-and-break-down-ai-agent-silos)
- [AGNTCY Documentation](https://docs.agntcy.org/)
- [Cisco Donates AGNTCY - The New Stack](https://thenewstack.io/cisco-donates-the-agntcy-project-to-the-linux-foundation/)

### NIST
- [NIST AI Agent Standards Initiative](https://www.nist.gov/caisi/ai-agent-standards-initiative)
- [NIST Announcement](https://www.nist.gov/news-events/news/2026/02/announcing-ai-agent-standards-initiative-interoperable-and-secure)

### OCI and Distribution
- [Demystifying OCI Specifications - Docker](https://www.docker.com/blog/demystifying-open-container-initiative-oci-specifications/)
- [OCI Artifacts Explained](https://oneuptime.com/blog/post/2025-12-08-oci-artifacts-explained/view)
- [Docker OCI Artifacts for AI Model Packaging](https://www.docker.com/blog/oci-artifacts-for-ai-model-packaging/)
- [OCI Image and Distribution Specs v1.1](https://opencontainers.org/posts/blog/2024-03-13-image-and-distribution-1-1/)

### WASM and Agent Execution
- [Introducing Wassette - Microsoft](https://opensource.microsoft.com/blog/2025/08/06/introducing-wassette-webassembly-based-tools-for-ai-agents)
- [WASM Native AI Runtimes](https://medium.com/wasm-radar/the-rise-of-wasm-native-runtimes-for-ai-tools-91b2da07b2ad)
- [Sandboxing Agentic AI with WebAssembly - NVIDIA](https://developer.nvidia.com/blog/sandboxing-agentic-ai-workflows-with-webassembly/)
- [WASI and Component Model Status](https://eunomia.dev/blog/2025/02/16/wasi-and-the-webassembly-component-model-current-status/)

### Registries and Marketplaces
- [Smithery.ai](https://smithery.ai/)
- [Glama.ai MCP Servers](https://glama.ai/mcp/servers)
- [Microsoft Magentic Marketplace](https://thenewstack.io/microsoft-launches-magentic-marketplace-for-ai-agents/)

### Package Distribution and Trust
- [Trusted Publishing on crates.io](https://alpha-omega.dev/blog/trusted-publishing-secure-rust-package-deployment-without-secrets/)
- [Trusted Publishing for npm](https://docs.npmjs.com/trusted-publishers/)
- [PyPI Trusted Publishers](https://blog.pypi.org/posts/2023-04-20-introducing-trusted-publishers/)
- [Trusted Publishing Benchmark - Trail of Bits](https://blog.trailofbits.com/2023/05/23/trusted-publishing-a-new-benchmark-for-packaging-security/)

### Terraform Registry
- [Terraform Module Registry Protocol](https://developer.hashicorp.com/terraform/internals/module-registry-protocol)
- [Publishing from Monorepo - Lessons Learned](https://dx.pagopa.it/blog/terraform-registry-journey)

### OpenAPI/AsyncAPI Governance
- [AsyncAPI Governance Motivation](https://www.asyncapi.com/blog/governance-motivation)
- [AsyncAPI Governance Overview](https://www.asyncapi.com/docs/community/020-governance-and-policies)

### Agent Security
- [NIST RFI on AI Agent Security](https://arxiv.org/html/2603.12230)
- [CELLMATE: Sandboxing Browser AI Agents](https://arxiv.org/pdf/2512.12594)
- [AI Agent Security - IBM](https://www.ibm.com/think/tutorials/ai-agent-security)
- [Hardening Agentic AI with WebAssembly](https://medium.com/@oracle_43885/hardening-agentic-ai-with-webassembly-69e5edd2c148)

### Agent Framework Comparison
- [Open Source AI Agent Frameworks Compared 2026](https://openagents.org/blog/posts/2026-02-23-open-source-ai-agent-frameworks-compared)
- [Agent Frameworks Comparison - DEV Community](https://dev.to/synsun/autogen-vs-langgraph-vs-crewai-which-agent-framework-actually-holds-up-in-2026-3fl8)

### Criticism and Discussion
- [The Problem with Agent Communication Protocols - InfoWorld](https://www.infoworld.com/article/4033863/the-problem-with-ai-agent-to-agent-communication-protocols.html)
- [MCP is a fad - HN Discussion](https://news.ycombinator.com/item?id=46552254)
- [Agent Discovery Trends - Google Cloud Community](https://medium.com/google-cloud/what-are-the-trends-in-agent-discoverability-and-interoperability-91865e098365)
- [5 Key Trends Shaping Agentic Development in 2026 - The New Stack](https://thenewstack.io/5-key-trends-shaping-agentic-development-in-2026/)
