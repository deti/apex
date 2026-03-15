# AI Agent Standards, Protocols, and Specifications: Comprehensive Research

**Date:** 2026-03-15
**Purpose:** Foundational research for RFC on agent distribution, definition, and interoperability

---

## Table of Contents

1. [Governance Bodies and Foundations](#1-governance-bodies-and-foundations)
2. [Agent Communication Protocols](#2-agent-communication-protocols)
3. [Agent Definition and Description Formats](#3-agent-definition-and-description-formats)
4. [Agent Discovery and Registry Standards](#4-agent-discovery-and-registry-standards)
5. [Agent-User Interaction Protocols](#5-agent-user-interaction-protocols)
6. [Framework-Specific Agent Formats](#6-framework-specific-agent-formats)
7. [Standards Body Work (W3C, IETF, IEEE)](#7-standards-body-work-w3c-ietf-ieee)
8. [Legacy Standards](#8-legacy-standards)
9. [Gap Analysis](#9-gap-analysis)
10. [Landscape Summary Table](#10-landscape-summary-table)

---

## 1. Governance Bodies and Foundations

### Agentic AI Foundation (AAIF)

- **URL:** https://aaif.io/
- **Status:** Active (founded December 2025)
- **Governance:** Directed fund under the Linux Foundation
- **Founding members:** Anthropic, OpenAI, Block
- **Platinum members:** AWS, Anthropic, Block, Bloomberg, Cloudflare, Google, Microsoft, OpenAI
- **Anchored projects:** MCP, AGENTS.md, goose
- **What it covers:** Neutral governance for agentic AI open standards; coordination of MCP evolution, agent manifest formats, and agent frameworks
- **What it misses:** No single unified specification yet; acts as an umbrella rather than prescribing a complete interoperability stack. Does not yet own A2A (which is under Linux Foundation separately).

---

## 2. Agent Communication Protocols

### 2.1 Agent2Agent Protocol (A2A) -- Google

- **URL:** https://a2a-protocol.org/ | https://github.com/a2aproject/A2A
- **Status:** Active, v0.3 (July 2025). Under Linux Foundation governance.
- **Spec version:** Release 0.3
- **What it covers:**
  - Agent-to-agent communication over JSON-RPC / HTTP
  - **Agent Card** (`.well-known/agent-card.json`) for capability discovery
  - Task lifecycle management (create, execute, status, artifacts)
  - Streaming via SSE
  - gRPC support (v0.3)
  - Signed security cards (v0.3)
  - Stateless interaction support (v0.2)
  - OpenAPI-like authentication schema
- **What it misses:**
  - No prescribed standard API for curated registries (discovery is underspecified)
  - No agent definition/creation format -- only runtime communication
  - No built-in payment or transaction primitives
  - Agent Card is discovery-only metadata, not a full agent definition language
- **Supporters:** 150+ organizations. ACP (IBM) merged into A2A as of Sept 2025.

### 2.2 Model Context Protocol (MCP) -- Anthropic / AAIF

- **URL:** https://modelcontextprotocol.io/ | Spec: https://modelcontextprotocol.io/specification/2025-11-25
- **Status:** Active, rapidly evolving. Donated to AAIF (Dec 2025).
- **What it covers:**
  - Host-to-Server protocol for connecting LLM applications to external tools/data
  - JSON-RPC over stdio/HTTP (Streamable HTTP)
  - Tool definitions, resource access, prompt templates
  - MCP Registry for server discovery
  - MCP Apps extension (SEP-1865) for interactive UI in conversations
- **2026 roadmap:**
  - Agent-to-agent communication (MCP servers acting as agents)
  - Stateless transport across load balancers (June 2026)
  - Enterprise readiness: audit trails, SSO, gateway behavior
- **What it misses:**
  - Currently host-to-server only, not agent-to-agent (planned for 2026)
  - No agent definition format
  - No agent identity/authentication framework (defers to transport layer)
  - Does not define agent capabilities or lifecycle

### 2.3 Agent Communication Protocol (ACP) -- IBM

- **URL:** https://agentcommunicationprotocol.dev/ | https://github.com/i-am-bee/acp
- **Status:** MERGED INTO A2A (September 2025). Winding down.
- **What it covered:**
  - REST-based agent-to-agent communication (simpler than A2A's JSON-RPC)
  - MimeType-based content identification
  - Agent Manifest for capability declaration
  - Standard HTTP tooling compatible (curl, Postman)
- **What it missed:** Now deprecated in favor of A2A.

### 2.4 Agent Protocol (agent-protocol.ai) -- AI Engineer Foundation / AGI Inc.

- **URL:** https://agentprotocol.ai/ | https://github.com/AI-Engineer-Foundation/agent-protocol
- **Status:** Active but low momentum compared to A2A/MCP
- **What it covers:**
  - REST API specification (OpenAPI 3.0) for interacting with agents
  - Core abstractions: Tasks, Steps, Artifacts
  - Endpoints: POST /ap/v1/agent/tasks, POST .../steps, GET .../artifacts
  - Standardized evaluation/benchmarking across agents
- **What it misses:**
  - No agent-to-agent communication (single agent API only)
  - No discovery mechanism
  - No agent definition format
  - Limited adoption compared to A2A

### 2.5 Agent Network Protocol (ANP)

- **URL:** https://agent-network-protocol.com/ | https://github.com/agent-network-protocol/AgentNetworkProtocol
- **Status:** Active, early stage. Led formation of W3C AI Agent Protocol Community Group.
- **What it covers:**
  - Three-layer architecture: Identity + Encrypted Comms, Meta-Protocol Negotiation, Application Protocol
  - Decentralized identity via W3C DID (did:wba method)
  - HTTPS-hosted DID documents
  - Meta-protocol layer for agents to negotiate communication protocols dynamically
  - Semantic web-based capability description
- **What it misses:**
  - Early stage, limited production adoption
  - Complex architecture may be over-engineered for simple use cases
  - No major vendor backing (compared to A2A/MCP)

### 2.6 AITP (Agent Interaction & Transaction Protocol) -- NEAR

- **URL:** https://aitp.dev/ | https://github.com/nearai/aitp
- **Status:** Active, pre-v1.0. Specification in progress.
- **What it covers:**
  - Agent-to-agent and user-to-agent communication
  - Chat Threads (OpenAI Assistant/Threads API compatible)
  - Structured Capabilities for payments, data sharing, UIs
  - Cross-trust-boundary security
  - Integration with NEAR AI Hub
- **What it misses:**
  - Pre-v1.0, not production-ready
  - Crypto/NEAR ecosystem oriented; unclear enterprise adoption path
  - Payment primitives tied to blockchain

### 2.7 Agora Protocol

- **URL:** https://agoraprotocol.org/
- **Status:** Academic/research stage (Oxford University, Oct 2024 paper)
- **What it covers:**
  - Fully decentralized agent communication
  - Meta-protocol: agents negotiate their own protocols using natural language
  - Hash-based protocol IDs (no central registration)
  - Agents can write and execute code to implement protocols on the fly
- **What it misses:**
  - Research prototype only
  - No production implementations
  - Relies heavily on LLM capabilities for protocol negotiation (fragile)

---

## 3. Agent Definition and Description Formats

### 3.1 Open Agent Specification (Agent Spec) -- Oracle

- **URL:** https://oracle.github.io/agent-spec/ | https://github.com/oracle/agent-spec
- **Status:** Active, v25.4.0. Published Oct 2025 (arXiv: 2510.04173).
- **What it covers:**
  - Framework-agnostic declarative language for defining agents and workflows
  - JSON/YAML serializable (JSON Schema based)
  - Components: Agents, Flows, Nodes (LLMNode, ToolNode), Tools
  - Input/output schemas (JSON Schema)
  - Multi-agent system composition
  - SDK (Python): build, validate, transform Agent Spec definitions
  - Reference runtime: WayFlow
  - Adapters for LangGraph, CrewAI, and other frameworks
  - AG-UI integration announced
- **What it misses:**
  - Oracle-driven; unclear community governance path
  - No discovery/registry mechanism
  - No runtime communication protocol (complements but does not replace A2A/MCP)
  - Memory and planning modules marked as "future extensions"

### 3.2 AGENTS.md -- OpenAI / AAIF

- **URL:** https://agents.md/ | https://github.com/agentsmd/agents.md
- **Status:** Active, widely adopted (60,000+ repos). Under AAIF governance.
- **What it covers:**
  - Markdown file format for guiding coding agents within repositories
  - Build steps, test commands, architecture, conventions, security guidance
  - Hierarchical: nearest file in directory tree takes precedence
  - No required structure -- standard Markdown
- **What it misses:**
  - Coding agent guidance only, not a general agent definition format
  - No machine-parseable schema (just Markdown prose)
  - No capability declarations, API definitions, or runtime behavior
  - Not suitable for agent-to-agent discovery or interoperability

### 3.3 Agent Skills -- Anthropic / OpenAI

- **URL:** https://agentskills.io/ (spec) | https://github.com/openai/skills (OpenAI catalog)
- **Status:** Active. Released by Anthropic Dec 2025, adopted by OpenAI for Codex/ChatGPT.
- **What it covers:**
  - SKILL.md file format: YAML frontmatter (name, description, version) + Markdown body (instructions)
  - Modular, reusable agent capabilities as self-contained directories
  - Skills discoverable via directory structure
  - Adopted by Claude, Codex CLI, ChatGPT
- **What it misses:**
  - Coding/task agent focus, not general agent interoperability
  - No runtime protocol
  - No capability negotiation
  - Directory-based packaging only

### 3.4 Agent Card (A2A)

- **URL:** https://a2a-protocol.org/latest/specification/
- **Status:** Active (part of A2A spec)
- **What it covers:**
  - JSON metadata: identity, capabilities, skills, service endpoint, auth requirements
  - Published at `.well-known/agent-card.json`
  - Discovery-oriented
- **What it misses:**
  - Discovery metadata only, not a full agent definition language
  - No internal agent logic, workflow, or tool definitions

### 3.5 JSON Agents / Portable Agent Manifest (PAM)

- **URL:** https://jsonagents.org/ | https://github.com/JSON-Agents/Standard
- **Status:** Early/draft
- **What it covers:**
  - Universal JSON-native standard for describing agents
  - Based on RFC 8259, JSON Schema 2020-12
  - Covers: capabilities, tools, runtimes, governance
  - Framework-agnostic, single manifest file
- **What it misses:**
  - Very early stage, minimal adoption
  - No major vendor backing
  - No runtime protocol or communication layer

### 3.6 Eclipse LMOS Agent Definition Language (ADL)

- **URL:** https://eclipse.dev/lmos/ | https://eclipse.dev/lmos/docs/multi_agent_system/agent_description/
- **Status:** Active (announced Oct 2025)
- **What it covers:**
  - Agent & Tool Description Format using JSON-LD
  - ADL for defining agent behavior (structured, model-neutral)
  - W3C DID-based identity/authentication
  - HTTP sub-protocol (REST) + WebSocket sub-protocol (real-time)
  - Protocol for tool discovery, agent collaboration, and integration
- **What it misses:**
  - Eclipse Foundation project; limited adoption outside telco/enterprise
  - Plans W3C standardization but not yet submitted
  - JSON-LD adds complexity vs plain JSON/YAML

### 3.7 Agent Manifest (Microsoft Copilot / Entra)

- **URL:** https://learn.microsoft.com/en-us/copilot/security/developer/agent-manifest
- **Status:** Active (Microsoft ecosystem)
- **What it covers:**
  - JSON metadata for Copilot agent registration
  - Agent card for Entra Agent Registry
  - Capabilities, skills, discovery metadata
  - Integration with Microsoft Agent Framework
- **What it misses:**
  - Microsoft-specific ecosystem
  - Not an open interoperability standard
  - Tied to Entra/Copilot platform

---

## 4. Agent Discovery and Registry Standards

### 4.1 A2A Agent Card Discovery

- **Mechanism:** `.well-known/agent-card.json` hosted at agent's base URL
- **Status:** Part of A2A spec (active)
- **Coverage:** Self-describing JSON card; decentralized (each agent hosts its own)
- **Gap:** No curated registry standard; no search/index mechanism

### 4.2 MCP Registry

- **URL:** Announced Sept 2025 (GitHub-based mcp.json)
- **Status:** Active, centralized
- **Coverage:** Public registry for MCP server discovery
- **Gap:** Centralized (GitHub-based); not suitable for decentralized agent ecosystems

### 4.3 GoDaddy Agent Name Service (ANS)

- **URL:** https://www.AgentNameRegistry.org
- **Status:** Active (API and standards site launched Nov 2025)
- **What it covers:**
  - DNS-based agent identity and discovery
  - PKI/X.509 cryptographic trust
  - Merkle tree-based Transparency Log
  - Protocol-agnostic adapter layer (works with A2A, MCP)
  - AgentID hostname format, capability registration
  - Sub-100ms lookup latency via DNS anycast
- **What it misses:**
  - GoDaddy-driven; unclear multi-vendor governance
  - Depends on DNS infrastructure (may not suit all agent topologies)
  - Identity-focused; does not define agent behavior or capabilities beyond metadata

### 4.4 Microsoft Entra Agent Registry

- **URL:** https://learn.microsoft.com/en-us/entra/agent-id/
- **Status:** Active (Microsoft ecosystem)
- **Coverage:** Agent card registration, capability metadata, SSO integration
- **Gap:** Microsoft-specific; not an open standard

### 4.5 agents.json (Web Discovery)

- **URL:** https://github.com/lando22/agents.json
- **Status:** Early proposal
- **Coverage:** Like robots.txt but for agents; describes UI interactions and site capabilities
- **Gap:** Focused on web UI automation, not general agent discovery

### 4.6 Wildcard AI agents.json

- **URL:** https://github.com/wild-card-ai/agents-json
- **Status:** Early proposal
- **Coverage:** Open spec for API/agent interaction contracts, built on OpenAPI
- **Gap:** Narrow focus on API contracts

---

## 5. Agent-User Interaction Protocols

### 5.1 AG-UI (Agent-User Interaction Protocol)

- **URL:** https://docs.ag-ui.com/ | https://github.com/ag-ui-protocol/ag-ui
- **Status:** Active. Originated from CopilotKit.
- **What it covers:**
  - Event-based streaming protocol between agent backend and frontend
  - ~16 standard event types: TEXT_MESSAGE, TOOL_CALL, STATE_DELTA, INTERRUPT
  - Transport agnostic: SSE, WebSockets, webhooks
  - SDKs for TypeScript and Python
  - Integration with LangGraph, CrewAI, Microsoft Agent Framework, Oracle Agent Spec
- **What it misses:**
  - Frontend/UI layer only; no agent-to-agent capabilities
  - No agent definition format
  - No discovery or registry

### 5.2 WebMCP (Web Model Context Protocol)

- **URL:** https://webmachinelearning.github.io/webmcp/ | https://github.com/webmachinelearning/webmcp
- **Status:** W3C Draft Community Group Report (Feb 2026). Chrome 146 Canary preview.
- **What it covers:**
  - Browser-native API (navigator.modelContext) for websites to expose structured tools to agents
  - Tool declarations with schemas, parameters, security boundaries
  - HTTPS required, human-in-the-loop confirmation, domain-level isolation
  - Created by engineers at Google and Microsoft
- **What it misses:**
  - Browser-only (client-side); does not cover server-side agent communication
  - Complementary to MCP (backend) rather than replacing it
  - Very early; only Chrome Canary preview

---

## 6. Framework-Specific Agent Formats

### 6.1 CrewAI

- **Agent format:** YAML (config/agents.yaml + config/tasks.yaml)
- **Fields:** role, goal, backstory, llm, verbose, tools, allow_delegation
- **Status:** Active, popular framework
- **Portability:** CrewAI-specific; no standard export to other frameworks (Agent Spec adapter exists)

### 6.2 LangGraph / LangChain

- **Agent format:** Code-first (Python). Directed graphs with StateGraph.
- **Components:** Nodes (agents/functions), Edges (flow), centralized state
- **Status:** Active, recommended by LangChain for all new agent work
- **Portability:** Code-defined; Agent Spec adapter exists for declarative interop

### 6.3 Microsoft Agent Framework (AutoGen + Semantic Kernel)

- **Agent format:** Code-first (Python/.NET). Graph-based workflows.
- **Components:** Agent interface, middleware, session management, hosted tools
- **Status:** Public preview (2025). AutoGen maintenance-only; new work in Agent Framework.
- **Portability:** AG-UI integration announced; otherwise Microsoft-ecosystem oriented

### 6.4 OpenAI Agents SDK

- **Agent format:** Code-first (Python/TypeScript)
- **Components:** Agents (instruction-driven), Tools (auto-schema), Handoffs, Guardrails
- **Status:** Active, open-source, provider-agnostic
- **Standards participation:** AGENTS.md, AAIF, Agent Skills adoption
- **Portability:** Provider-agnostic models supported; no declarative export format

### 6.5 Google ADK (Agent Development Kit)

- **Agent format:** Code-first (Python/TypeScript)
- **Components:** Agent, Tool (Python functions with type hints), Callbacks, Sessions
- **Status:** Active, optimized for Gemini but model-agnostic
- **Standards participation:** A2A integration native
- **Portability:** Code-defined; no declarative format

---

## 7. Standards Body Work (W3C, IETF, IEEE)

### 7.1 W3C AI Agent Protocol Community Group

- **Status:** Active (initiated by ANP community, 2025)
- **Focus:** Protocols for agents to find, identify, and collaborate on the web
- **Expected specs:** 2026-2027
- **Notable work:** WebMCP (Draft Community Group Report, Feb 2026)

### 7.2 W3C Web & AI Interest Group

- **Status:** Active (chartered group)
- **Focus:** Exploring AI impact on web platform
- **Notable work:** TPAC 2025 AI sessions

### 7.3 IETF Internet-Drafts

- **draft-liu-agent-context-protocol-00:** Agent Context Protocol -- JSON/HTTP standard for agent context exchange
- **draft-klrc-aiagent-auth-00:** AI Agent Authentication/Authorization (March 2026) -- composing WIMSE, SPIFFE, OAuth 2.0
- **draft-hw-ai-agent-6g-00:** Agent protocols for 6G networks
- **Status:** All early Internet-Drafts; none adopted as RFCs yet
- **IETF blog (2025):** Actively identifying which agentic AI communications need standardization

### 7.4 IEEE FIPA (see Legacy Standards below)

---

## 8. Legacy Standards

### 8.1 FIPA-ACL (Foundation for Intelligent Physical Agents)

- **URL:** http://www.fipa.org/repository/aclspecs.html
- **Status:** Historically complete; maintained under IEEE Computer Society (since 2005). No active development.
- **What it covered:**
  - Agent Communication Language (FIPA-ACL): message structure with performatives (request, inform, query-if, confirm, etc.)
  - Agent Management Reference Model (agent platform, directory facilitator, agent management system)
  - Content language + ontology specification
  - Interaction protocols (request, contract-net, propose, etc.)
  - Agent mobility
- **Historical significance:** First comprehensive multi-agent interoperability standard (1996-2005)
- **What it misses for modern use:**
  - Designed for traditional software agents, not LLM-based agents
  - No concept of tools, prompts, or generative capabilities
  - No streaming, no modern transport (HTTP/2, gRPC, SSE, WebSockets)
  - No semantic web integration
  - Rigid ontology system vs. modern schema approaches (JSON Schema)
- **Relevance today:** Conceptual ancestor. Performative model and agent management concepts influenced A2A, ACP, and ANP designs.

---

## 9. Gap Analysis

### What EXISTS (well-covered areas):

| Area | Leading Standard(s) |
|------|---------------------|
| Agent-to-agent communication | A2A (Google/Linux Foundation) |
| Agent-to-tool/data integration | MCP (Anthropic/AAIF) |
| Agent-to-user frontend | AG-UI (CopilotKit) |
| Agent-to-browser tools | WebMCP (W3C draft) |
| Coding agent repo guidance | AGENTS.md (AAIF) |
| Modular agent skills/capabilities | Agent Skills (Anthropic, adopted by OpenAI) |
| Agent definition/workflow language | Agent Spec (Oracle) |
| DNS-based agent identity | ANS (GoDaddy) |
| Decentralized agent identity | ANP (W3C CG, DID-based) |
| Agent payments | x402 (Coinbase/Cloudflare), AITP (NEAR) |

### What is MISSING or FRAGMENTED:

1. **Unified agent definition format adopted across all major vendors.** Agent Spec (Oracle), JSON Agents (PAM), LMOS ADL, and framework-specific formats all compete. No single winner.

2. **Standard agent registry / directory protocol.** A2A Agent Card is self-hosted discovery. MCP Registry is centralized/GitHub. ANS is DNS-based. Microsoft Entra is proprietary. No universal federated registry standard.

3. **Agent identity standard.** ANP uses W3C DID. ANS uses DNS/PKI. A2A uses OpenAPI-like auth. IETF draft-klrc proposes OAuth/SPIFFE composition. No convergence.

4. **Agent lifecycle management.** No standard for agent versioning, deployment, health monitoring, deprecation, or migration across platforms.

5. **Agent capability negotiation.** ANP has a meta-protocol layer; A2A has Agent Cards; but no standard for agents to dynamically negotiate protocols or capabilities at runtime.

6. **Agent trust and reputation.** GoDaddy ANS has transparency logs; no broader standard for agent reputation, ratings, or trust scoring.

7. **Cross-protocol bridging.** MCP (tool integration) and A2A (agent communication) are complementary but no standard bridge protocol exists for composing them.

8. **Agent packaging and distribution.** No standard for packaging an agent as a distributable unit (container image, WASM module, or archive) with manifest, dependencies, and runtime requirements.

9. **Agent governance and compliance.** LMOS mentions governance; JSON Agents PAM includes governance fields; but no comprehensive standard for agent audit trails, regulatory compliance, or safety constraints.

10. **Semantic capability description.** LMOS uses JSON-LD; FIPA had ontologies; modern protocols mostly use plain-text descriptions. No standard machine-readable capability taxonomy.

---

## 10. Landscape Summary Table

| Name | Type | Status | Governance | Transport | Format | Discovery |
|------|------|--------|------------|-----------|--------|-----------|
| **A2A** | Agent-to-agent protocol | Active v0.3 | Linux Foundation | JSON-RPC/HTTP, gRPC, SSE | Agent Card JSON | .well-known |
| **MCP** | Agent-to-tool protocol | Active (2025-11-25 spec) | AAIF/Linux Foundation | JSON-RPC/stdio/HTTP | Tool definitions JSON | MCP Registry |
| **Agent Protocol** | Agent API spec | Active (low momentum) | AI Engineer Foundation | REST/OpenAPI 3.0 | Tasks/Steps/Artifacts | None |
| **ACP** | Agent-to-agent protocol | MERGED into A2A | Linux Foundation | REST/HTTP | Agent Manifest JSON | None |
| **ANP** | Decentralized agent protocol | Active (early) | W3C CG | HTTPS + DID | DID documents | DID resolution |
| **AITP** | Agent interaction + transactions | Pre-v1.0 | NEAR | Chat Threads | Capabilities JSON | None |
| **Agora** | Meta-protocol | Research | Academic (Oxford) | Any (negotiated) | Natural language | Decentralized |
| **Agent Spec** | Agent definition language | Active v25.4.0 | Oracle | N/A (declarative) | JSON/YAML | None |
| **AGENTS.md** | Coding agent guidance | Active (60K+ repos) | AAIF | N/A (file) | Markdown | Directory tree |
| **Agent Skills** | Modular capabilities | Active | AAIF (de facto) | N/A (file) | SKILL.md (YAML+MD) | Directory |
| **JSON Agents (PAM)** | Agent manifest format | Draft | Independent | N/A | JSON Schema 2020-12 | None |
| **LMOS ADL** | Agent definition + protocol | Active | Eclipse Foundation | HTTP + WebSocket | JSON-LD | DID-based |
| **AG-UI** | Agent-to-frontend protocol | Active | CopilotKit | SSE/WS/webhooks | Event stream JSON | None |
| **WebMCP** | Browser agent API | W3C Draft (Feb 2026) | W3C CG | navigator.modelContext | Tool declarations | In-page |
| **ANS** | Agent identity + discovery | Active | GoDaddy | DNS | AgentID format | DNS lookup |
| **FIPA-ACL** | Agent communication language | Legacy (IEEE) | IEEE CS | IIOP/HTTP (dated) | ACL messages | Directory Facilitator |
| **CrewAI** | Framework agent format | Active | CrewAI | N/A | YAML | N/A |
| **LangGraph** | Framework agent format | Active | LangChain | N/A | Code (Python) | N/A |
| **MS Agent Framework** | Framework agent format | Preview | Microsoft | N/A | Code (Python/.NET) | Entra Registry |
| **OpenAI Agents SDK** | Framework agent format | Active | OpenAI | N/A | Code (Python/TS) | N/A |
| **Google ADK** | Framework agent format | Active | Google | N/A | Code (Python/TS) | A2A native |

---

## Key Observations

1. **The stack is converging into layers:** MCP (tools/data) + A2A (agent-to-agent) + AG-UI (frontend) + WebMCP (browser). Each targets a different communication boundary.

2. **Two competing approaches to agent definition:** Declarative (Agent Spec, JSON Agents, LMOS ADL) vs. code-first (every major framework). The declarative formats are younger and less adopted.

3. **AAIF is the center of gravity** for standards governance, hosting MCP, AGENTS.md, and Agent Skills under the Linux Foundation with buy-in from Anthropic, OpenAI, Google, Microsoft, and AWS.

4. **A2A is winning the agent-to-agent protocol race.** ACP merged into it; 150+ supporting organizations; Linux Foundation governance; gRPC + REST + SSE transport options.

5. **No one has solved agent packaging/distribution.** This is the clearest gap -- there is no OCI-equivalent for agents. Agents are distributed as code (pip/npm packages), API endpoints, or platform-specific deployments.

6. **Identity is fragmented.** DID (ANP), DNS/PKI (ANS), OAuth (A2A/IETF), platform-specific (Entra) -- all compete with no convergence path.

7. **The "agent manifest" space is crowded but immature.** Agent Card (A2A), Agent Manifest (ACP/Microsoft), PAM (JSON Agents), ADL (LMOS), Agent Spec (Oracle) -- each covers slightly different metadata. None is dominant.

8. **FIPA's conceptual legacy persists** in the performative model (request/inform/confirm maps to A2A task lifecycle), agent management reference model (maps to modern registries), and directory facilitator concept (maps to discovery).
