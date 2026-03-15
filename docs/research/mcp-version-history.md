# MCP (Model Context Protocol) Complete Version History

Research date: 2026-03-15

---

## Version Timeline

| Spec Version | Release Date | Protocol Version String | Status |
|---|---|---|---|
| 2024-11-05 | 2024-11-05 | `"2024-11-05"` | Superseded |
| 2025-03-26 | 2025-03-26 | `"2025-03-26"` | Superseded |
| 2025-06-18 | 2025-06-18 | `"2025-06-18"` | Superseded |
| 2025-11-25 | 2025-11-25 | `"2025-11-25"` | Current stable |
| ~2026-06 (tentative) | TBD | TBD | Planned |

The protocol version string is exchanged in the `initialize` handshake via
`InitializeRequest.params.protocolVersion` and `InitializeResult.protocolVersion`.

---

## Version 1: 2024-11-05 (Initial Release)

The foundational spec. Established the core architecture.

### Core Architecture
- JSON-RPC 2.0 message format
- Stateful connections
- Three roles: Host, Client, Server
- Capability negotiation during `initialize` handshake

### Transports
- **stdio**: stdin/stdout for local process communication
- **HTTP with SSE**: HTTP POST for client-to-server, Server-Sent Events for server-to-client

### ServerCapabilities
```typescript
interface ServerCapabilities {
  experimental?: { [key: string]: object }
  logging?: {}                    // log message support
  prompts?: { listChanged?: boolean }
  resources?: { subscribe?: boolean; listChanged?: boolean }
  tools?: { listChanged?: boolean }
}
```

### ClientCapabilities
```typescript
interface ClientCapabilities {
  experimental?: { [key: string]: object }
  roots?: { listChanged?: boolean }
  sampling?: {}
}
```

### Server Features
- **Resources**: URI-addressed context data (`Resource { uri, name, description?, mimeType? }`)
- **Prompts**: Templated messages (`Prompt { name, description?, arguments? }`)
- **Tools**: Callable functions (`Tool { name, description?, inputSchema }`)

### Client Features
- **Sampling**: Server-initiated LLM requests (`CreateMessageRequest`)
- **Roots**: Filesystem boundary declarations

### Content Types
- `TextContent { type: "text", text: string }`
- `ImageContent { type: "image", data: string, mimeType: string }`
- `EmbeddedResource { type: "resource", resource: ... }`

### Utilities
- Progress notifications (`ProgressNotification { progressToken, progress, total? }`)
- Cancellation (`CancelledNotification`)
- Logging (`LoggingMessageNotification`)
- Ping/pong keepalive

---

## Version 2: 2025-03-26 (Auth + Streamable HTTP)

### Breaking Changes
- **Transport replacement**: HTTP+SSE transport replaced by **Streamable HTTP**
  - Client sends JSON-RPC over HTTP POST
  - Server can respond with single JSON or upgrade to SSE stream
  - Bidirectional communication in a single HTTP request/response cycle
  - Session management via `Mcp-Session-Id` header

### Major Additions

#### OAuth 2.1 Authorization Framework (NEW)
- Full authorization section added to spec
- OAuth 2.1 with PKCE flows
- Dynamic client registration
- Token-based access control
- Authorization server discovery

#### JSON-RPC Batching (NEW -- later removed in 2025-06-18)
- Support for batched JSON-RPC requests per the JSON-RPC spec

#### Tool Annotations (NEW)
```typescript
interface ToolAnnotations {
  title?: string           // Human-readable title
  readOnlyHint?: boolean   // true = tool does not modify state (default: false)
  destructiveHint?: boolean // true = tool may perform destructive operations (default: true)
  idempotentHint?: boolean // true = repeated calls with same args have same effect (default: false)
  openWorldHint?: boolean  // true = tool interacts with external world (default: true)
}
```
Added to `Tool` interface:
```typescript
interface Tool {
  name: string
  description?: string
  inputSchema: { type: "object", properties?: ..., required?: ... }
  annotations?: ToolAnnotations   // NEW
}
```

#### AudioContent (NEW)
```typescript
interface AudioContent {
  type: "audio"
  data: string       // base64-encoded audio
  mimeType: string   // e.g., "audio/wav"
}
```

#### ProgressNotification Enhancement
```typescript
interface ProgressNotification {
  progressToken: ProgressToken
  progress: number
  total?: number
  message?: string   // NEW - descriptive status text
}
```

#### Completions Capability (NEW)
- `completions` capability added to ServerCapabilities to indicate support for
  argument autocompletion suggestions

### ServerCapabilities (2025-03-26)
```typescript
interface ServerCapabilities {
  experimental?: { [key: string]: object }
  logging?: {}
  completions?: {}         // NEW
  prompts?: { listChanged?: boolean }
  resources?: { subscribe?: boolean; listChanged?: boolean }
  tools?: { listChanged?: boolean }
}
```

---

## Version 3: 2025-06-18 (Structured Output + Elicitation)

### Breaking Changes
- **JSON-RPC batching REMOVED** (added in 2025-03-26, removed here)
- **Lifecycle operation compliance**: changed from SHOULD to MUST
- **MCP-Protocol-Version header**: REQUIRED in all HTTP requests after handshake

### Major Additions

#### Structured Tool Output (NEW)
```typescript
interface Tool {
  name: string
  description?: string
  inputSchema: { type: "object", properties?: ..., required?: ... }
  outputSchema?: object    // NEW - JSON Schema for structured output
  annotations?: ToolAnnotations
}

interface CallToolResult {
  content: (TextContent | ImageContent | AudioContent | EmbeddedResource)[]
  structuredContent?: object   // NEW - matches outputSchema
  isError?: boolean
}
```

#### Elicitation (NEW Client Feature)
Server can request information from the user via the client:
```typescript
// Server sends: elicitation/create request
interface ElicitRequest {
  message: string
  requestedSchema: {
    type: "object"
    properties: Record<string, ElicitFieldSchema>
    required?: string[]
  }
}

// Client responds:
interface ElicitResult {
  action: "accept" | "decline" | "cancel"
  content?: object   // matches requestedSchema
}
```

#### Resource Links in Tool Results (NEW)
```typescript
interface ResourceLink {
  type: "resource_link"
  uri: string
  name: string
  description?: string
  mimeType?: string
}
```
Added to `CallToolResult.content` array options.

#### title Field (NEW across multiple interfaces)
```typescript
interface Tool { title?: string; name: string; ... }     // NEW title
interface Resource { title?: string; name: string; ... }  // NEW title
interface Prompt { title?: string; name: string; ... }    // NEW title
interface ResourceTemplate { title?: string; ... }        // NEW title
```
`name` became a programmatic identifier; `title` is the human-friendly display name.

#### _meta Field Extension
`_meta` field added to additional interface types for extensibility metadata.

#### CompletionRequest Enhancement
```typescript
interface CompletionRequest {
  ref: ResourceReference | PromptReference
  argument: { name: string; value: string }
  context?: Record<string, string>   // NEW - previously resolved variables
}
```

### OAuth Enhancements
- MCP servers classified as OAuth **Resource Servers**
- Protected resource metadata discovery added
- RFC 8707 Resource Indicators required for MCP clients
- Enhanced security considerations documentation
- New Security Best Practices page

### ClientCapabilities (2025-06-18)
```typescript
interface ClientCapabilities {
  experimental?: { [key: string]: object }
  roots?: { listChanged?: boolean }
  sampling?: {}
  elicitation?: {}   // NEW
}
```

---

## Version 4: 2025-11-25 (Tasks + Icons + Auth Simplification)

### No Breaking Changes
This release maintains full backward compatibility.

### Major Additions

#### Tasks (EXPERIMENTAL)
New abstraction for tracking durable server work (SEP-1686):
```typescript
// Task states: "working" | "input_required" | "completed" | "failed" | "cancelled"
interface Task {
  id: string
  status: TaskStatus
  result?: object
  // ... additional lifecycle fields
}
```
- Any request can be augmented with a task
- Clients can poll for status and retrieve results after completion
- Session-based access control for task isolation
- Launched as experimental capability

#### Icons (NEW metadata)
Servers can expose icons for tools, resources, resource templates, and prompts (SEP-973):
```typescript
interface Tool {
  name: string
  title?: string
  description?: string
  inputSchema: ...
  outputSchema?: ...
  annotations?: ToolAnnotations
  icon?: string    // NEW - icon URI
}
// Same `icon` field added to Resource, ResourceTemplate, Prompt
```

#### OAuth Client ID Metadata Documents (NEW)
Replaces complex Dynamic Client Registration (SEP-991):
- Clients provide a client ID that points to a JSON document describing client properties
- Eliminates need for OAuth proxies or manual IT registration

#### Enhanced Authorization Server Discovery
- OpenID Connect Discovery 1.0 support (PR #797)
- Incremental scope consent via `WWW-Authenticate` (SEP-835)
- OAuth Protected Resource Metadata aligned with RFC 9728

#### URL Mode Elicitation (NEW)
Secure out-of-band credential handling (SEP-1036):
- Users authenticate through browser-based OAuth flows
- Credentials never exposed to MCP client
- Supports PCI-compliant payment processing

#### ElicitResult / EnumSchema Updates (SEP-1330)
- More standards-based approach
- Supports titled, untitled, single-select, and multi-select enums
- Default values for all primitive types (string, number, enum)

#### Sampling with Tools (SEP-1577)
```typescript
interface CreateMessageRequest {
  // ... existing fields ...
  tools?: Tool[]         // NEW - tool definitions for sampling
  toolChoice?: ...       // NEW - tool selection strategy
}
```
Enables MCP servers to run agentic loops using client-provided LLM.

#### Implementation Description (NEW)
```typescript
interface Implementation {
  name: string
  version: string
  description?: string   // NEW - human-readable context
}
```

#### Tool Name Guidance (SEP-986)
Standardized tool name formatting rules.

#### Schema Changes
- Request payloads decoupled from RPC method definitions into standalone parameter schemas (SEP-1319)
- JSON Schema 2020-12 established as default dialect (SEP-1613)

#### Transport Refinements
- Servers using stdio may use stderr for all logging types (not just errors)
- HTTP 403 Forbidden required for invalid Origin headers in Streamable HTTP
- SSE polling support: servers can disconnect at will (SEP-1699)
- GET stream resumption always via GET regardless of stream origin

---

## Field-Level Diff Summary (Cumulative)

### Tool Interface Evolution

| Field | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| `name` | yes | yes | yes (programmatic ID) | yes |
| `description` | optional | optional | optional | optional |
| `inputSchema` | yes | yes | yes | yes |
| `annotations` | -- | ADDED | yes | yes |
| `outputSchema` | -- | -- | ADDED | yes |
| `title` | -- | -- | ADDED | yes |
| `icon` | -- | -- | -- | ADDED |

### Content Types Evolution

| Type | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| `TextContent` | yes | yes | yes | yes |
| `ImageContent` | yes | yes | yes | yes |
| `EmbeddedResource` | yes | yes | yes | yes |
| `AudioContent` | -- | ADDED | yes | yes |
| `ResourceLink` | -- | -- | ADDED | yes |

### ServerCapabilities Evolution

| Field | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| `experimental` | optional | optional | optional | optional |
| `logging` | optional | optional | optional | optional |
| `prompts` | optional | optional | optional | optional |
| `resources` | optional | optional | optional | optional |
| `tools` | optional | optional | optional | optional |
| `completions` | -- | ADDED | yes | yes |

### ClientCapabilities Evolution

| Field | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| `experimental` | optional | optional | optional | optional |
| `roots` | optional | optional | optional | optional |
| `sampling` | optional | optional | optional | optional |
| `elicitation` | -- | -- | ADDED | yes |

### Transport Evolution

| Transport | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| stdio | yes | yes | yes | yes |
| HTTP+SSE | yes | REMOVED | -- | -- |
| Streamable HTTP | -- | ADDED | yes | yes |
| JSON-RPC batching | -- | ADDED | REMOVED | -- |
| `MCP-Protocol-Version` header | -- | -- | REQUIRED | yes |
| `Mcp-Session-Id` header | -- | ADDED | yes | yes |

### CallToolResult Evolution

| Field | 2024-11-05 | 2025-03-26 | 2025-06-18 | 2025-11-25 |
|---|---|---|---|---|
| `content` | yes | yes | yes (+ ResourceLink) | yes |
| `isError` | optional | optional | optional | optional |
| `structuredContent` | -- | -- | ADDED | yes |
| `_meta` | -- | -- | ADDED | yes |

---

## 2026 Roadmap

Updated 2026-03-05. Organized by Working Group, not release milestones.

### Priority 1: Transport Evolution and Scalability (Transports WG)
- Evolve Streamable HTTP to run **statelessly** across multiple server instances
- Scalable session handling: create, resume, migrate sessions transparently
- **MCP Server Cards**: `/.well-known/mcp.json` for capability discovery without connecting
- Remove `initialize` handshake; include shared info with each request/response
- Expose routing info (RPC method, tool names) via HTTP paths/headers instead of JSON body parsing
- Replace general GET streams with explicit subscription streams + TTL/ETags
- NO new official transports this cycle
- **Target**: SEPs finalized Q1 2026; spec release ~June 2026

### Priority 2: Agent Communication (Agents WG)
- Harden Tasks primitive (SEP-1686) for production
- Retry semantics for transient task failures
- Expiry policies for completed task results
- Additional lifecycle gaps to be triaged from production deployments

### Priority 3: Governance Maturation (Governance WG)
- Contributor Ladder SEP (participant -> contributor -> facilitator -> lead -> core maintainer)
- Delegation model: WGs accept SEPs in their domain without full core review
- Charter template for all WGs/IGs (scope, deliverables, success criteria, retirement)

### Priority 4: Enterprise Readiness (Enterprise WG -- forming)
- Audit trails and observability
- SSO-integrated auth (Cross-App Access / xaa.dev)
- Gateway and proxy pattern standardization
- Configuration portability across MCP clients
- Likely extensions rather than core spec changes

### On the Horizon (lower priority)
- Triggers and event-driven updates (webhooks)
- Streamed/reference-based results (incremental output)
- Security: DPoP (SEP-1932), Workload Identity Federation (SEP-1933)
- Extensions ecosystem maturation (ext-auth, ext-apps, Skills primitive)

---

## Active / Notable SEPs

| SEP | Title | Status | Area |
|---|---|---|---|
| SEP-1686 | Tasks (durable request tracking) | Landed (experimental) in 2025-11-25 | Agent Communication |
| SEP-973 | Icons for tools/resources/prompts | Landed in 2025-11-25 | DX |
| SEP-991 | OAuth Client ID Metadata Documents | Landed in 2025-11-25 | Auth |
| SEP-1577 | Tool calling in sampling | Landed in 2025-11-25 | Agent Communication |
| SEP-1036 | URL mode elicitation | Landed in 2025-11-25 | Client Features |
| SEP-1330 | ElicitResult/EnumSchema standards alignment | Landed in 2025-11-25 | Client Features |
| SEP-986 | Tool name guidance | Landed in 2025-11-25 | DX |
| SEP-1319 | Decouple request payloads from RPC methods | Landed in 2025-11-25 | Schema |
| SEP-1613 | JSON Schema 2020-12 default dialect | Landed in 2025-11-25 | Schema |
| SEP-1699 | SSE polling via server disconnect | Landed in 2025-11-25 | Transport |
| SEP-1730 | SDK tiering system | Landed in 2025-11-25 | Governance |
| SEP-1302 | Working Groups and Interest Groups | Landed in 2025-11-25 | Governance |
| SEP-1724 | Extensions framework | In progress | Extensions |
| SEP-1865 | MCP Apps (interactive UIs) | In progress / extension | Extensions |
| SEP-1932 | DPoP (proof-of-possession tokens) | In progress | Security |
| SEP-1933 | Workload Identity Federation | In progress | Security |
| SEP-2085 | Succession and amendment procedures | Landed | Governance |
| SEP-2133 | Experimental extension repos | Landed | Extensions |

---

## Key Takeaways for Protocol Drift Simulation

1. **Transport is the biggest source of breaking changes**: HTTP+SSE -> Streamable HTTP (2025-03-26), batching added then removed (2025-03-26 -> 2025-06-18).

2. **Additive field growth is constant**: Every version adds optional fields (`annotations`, `outputSchema`, `title`, `icon`, `structuredContent`, `_meta`, `description` on Implementation). Old clients that ignore unknown fields remain compatible.

3. **Capability flags gate features**: New features (elicitation, completions, tasks) are gated behind capability declarations in the handshake. Servers/clients that don't declare them are unaffected.

4. **The next major structural change** is the move to stateless transport (~June 2026), which would remove the `initialize` handshake entirely and change how sessions work. This is the biggest upcoming breaking change.

5. **Schema organization changed**: SEP-1319 decoupled request payloads from RPC method definitions into standalone parameter schemas. This affects code generators and SDK implementations.

6. **Auth evolution has been continuous**: No auth (2024-11-05) -> OAuth 2.1 + Dynamic Client Registration (2025-03-26) -> Resource Server classification + RFC 8707 (2025-06-18) -> Client ID Metadata Documents replacing Dynamic Registration (2025-11-25).
