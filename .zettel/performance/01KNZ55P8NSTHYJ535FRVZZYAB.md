---
id: 01KNZ55P8NSTHYJ535FRVZZYAB
title: graphql-faker and EasyGraphQL — Schema-Driven Mock Data and Test Generation
type: literature
tags: [graphql, graphql-faker, easygraphql, test-generation, mock-data, introspection]
links:
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.989454+00:00
modified: 2026-04-11T20:54:11.989459+00:00
source: "https://github.com/APIs-guru/graphql-faker"
---

# graphql-faker and EasyGraphQL — Schema-Driven Test Generation for GraphQL

Two tools worth knowing in the GraphQL test-generation corner.

## graphql-faker (APIs-guru/graphql-faker)

A tool that extends or mocks a GraphQL API with faked data driven by schema directives. The engineer annotates the SDL (or a wrapper SDL on top of an introspected schema) with `@fake`, `@examples`, and `@listLength` directives and graphql-faker serves a running GraphQL endpoint that responds with synthesised data.

```graphql
type User {
  id: ID!
  firstName: String @fake(type: firstName)
  email: String @fake(type: email)
  friends: [User] @listLength(min: 2, max: 10)
}
```

This is valuable in two ways:

1. **Frontend/mobile decoupling.** Client teams can develop against a believable GraphQL mock while the backend is still being built.
2. **Test data generation for load testing.** Because graphql-faker's responses look like real data (faker.js names, addresses, ids), you can point a load generator at it and get realistic-size responses without touching the real backend.

### Limits for performance testing

- **It's a server, not a client generator.** graphql-faker generates *responses*, not *queries*. For load testing a real backend you still need something that generates diverse GraphQL queries.
- **Field distributions are shallow.** `@fake(type: firstName)` gives names but not a realistic *distribution* of names (no Zipfian, no locality). For performance characteristics that depend on string length distribution, this is already better than pure-random but still not a production histogram.
- **Nested list length is a hand-set integer range.** If a real user sometimes has 2 friends and sometimes 2,000, `@listLength(min: 2, max: 10)` hides the performance-relevant tail.

## EasyGraphQL (EasyGraphQL/easygraphql-tester)

A Node library for testing GraphQL queries and mutations against a schema. Two modes: `.tester` as an assertion utility (does this query validate against the schema?) and `.mock` to return a mocked result for a query. It is closer to a unit-test helper than a test-generation tool — it doesn't generate queries, it validates them.

It is useful as a *building block* for a perf-test generator that needs to verify generated queries are schema-conformant before firing them at the real backend.

## GraphQL performance generation — the real gap

The GraphQL space is worse off than REST for load-test generation, and the gap is structural:

1. **Query selection sets are the performance variable.** A GraphQL endpoint returns exactly what the client asks for. Two requests to the same endpoint with different selection sets can differ in latency by 10–100×. A load-test generator that only fires one canned query per endpoint is almost useless.
2. **Nesting depth drives resolver fan-out.** Deep queries trigger N+1 resolver calls and are the most common cause of GraphQL perf incidents. A generator that does not control depth cannot reproduce the issue.
3. **Persisted query replay is the practical answer.** Most mature GraphQL deployments (Apollo, Shopify) use persisted queries where the client sends a hash, not the query text. The query corpus *is the workload*. Replaying persisted-query logs is the only realistic GraphQL load test — and it's exactly the "generation from production traffic" approach, not schema-driven generation.
4. **Tools like Apollo's `mocked-schema`** and Apollo Server's built-in mocking are the Apollo ecosystem's graphql-faker equivalent, but again serve responses, not generate queries.

## What a good GraphQL load-test generator would do

- Ingest the schema and a corpus of observed persisted queries.
- Learn the distribution over query shapes (which fields, which depth, which arguments).
- Sample new queries from that distribution at a controlled rate.
- Optionally, mutate sampled queries to stress depth/fan-out (the GraphQL DoS attack literature is directly relevant here — see Cheney/Hartig's "GraphQL cost analysis" and the `graphql-depth-limit`, `graphql-query-complexity` libraries).

No tool does this end-to-end in open source as of early 2024. The closest is Schemathesis, which supports GraphQL schema-based generation but does not learn from production corpora.

## Citations

- https://github.com/APIs-guru/graphql-faker
- https://www.npmjs.com/package/graphql-faker
- https://github.com/EasyGraphQL/easygraphql-tester
- Apollo mocking: https://www.apollographql.com/docs/apollo-server/testing/mocking
- GraphQL depth/cost literature: https://github.com/stems/graphql-depth-limit (first widely used complexity limiter)