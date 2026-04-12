---
id: 01KNZ706EPCBY2YCY9VDDG2WMT
title: kloadgen — JMeter Plugin for Kafka Load Testing with Schema Registry
type: literature
tags: [kloadgen, kafka, jmeter, avro, protobuf, schema-registry, event-driven, load-testing]
links: []
created: 2026-04-11T21:26:09.110826+00:00
modified: 2026-04-11T21:26:09.110832+00:00
source: "https://github.com/corunet/kloadgen"
---

# kloadgen — JMeter Plugin for Realistic Kafka Load Testing

kloadgen is an open-source JMeter plugin, originally from Corunet (now Sngular), that provides first-class Kafka production load testing with schema-registry integration. It is, as of 2024, the most widely recommended open-source tool for driving realistic Kafka load from a schema.

## What it does

kloadgen adds JMeter samplers for:

- **Kafka Producer.** Produce messages to a topic at a rate controlled by JMeter's thread group.
- **Kafka Consumer.** Consume messages for latency measurement and back-pressure testing.

And — this is the differentiator — it understands **schema-registry-backed schemas**:

- **Avro** with Confluent Schema Registry or Apicurio.
- **JSON Schema** with schema-registry lookup.
- **Protobuf** with schema-registry.

For each schema, kloadgen lets you configure per-field value generators (random, list, sequence, faker-style) and then *validates each produced message against the schema* before sending. Invalid messages are rejected. This is correct-by-construction message generation from a schema.

## Why this matters

The AsyncAPI note describes the general event-driven test-generation gap. kloadgen is the *only* widely used tool that covers a non-trivial chunk of it. It's not AsyncAPI-aware — it works at the lower level of schema registry and Kafka topics directly — but the result is similar: you can generate schema-conformant traffic at scale without writing per-message serialisation code.

Key use cases:

1. **Throughput benchmarks for Kafka producers/consumers** with realistic message shapes.
2. **Schema evolution testing** — send new-version messages to a consumer that only knows the old schema, check behaviour.
3. **Partitioning-behaviour tests** — configure key generators with specific distributions to trigger partition hot spots deliberately.
4. **Consumer-lag tests** — pair a producer running at rate λ with a consumer running at rate μ < λ to induce lag and measure recovery.

## Adversarial reading

1. **JMeter overhead.** kloadgen inherits JMeter's heavy GUI, its verbose JMX files, and its per-thread model. High-RPS Kafka testing (100k+ messages/s) hits JMeter's ceiling before it hits Kafka's. For that scale you'd use Kafka's own `kafka-producer-perf-test.sh` or a custom Go producer, both of which lack schema awareness.
2. **Distribution control is shallow.** You can pick values from lists or generators but not from observed production distributions. The same data-realism gap every schema-driven tool has.
3. **Header support is basic.** Kafka record headers (used for routing in some topologies) get less first-class treatment than key/value.
4. **Avro-first.** Protobuf and JSON Schema work but the Avro path is clearly the best-tested. Teams on non-Avro stacks report more friction.
5. **Consumer-side measurement is indirect.** For end-to-end latency you need producer-side timestamp in the message plus consumer-side subtract. kloadgen helps with the producer; the consumer end-to-end latency measurement is on you to instrument.
6. **No CBMG for event topologies.** Real event workloads have causal structure (event A triggers event B upstream, which triggers event C downstream). kloadgen treats producers and consumers as independent; modelling causal cascades is out of scope.

## The overall Kafka load-testing landscape

- **kloadgen (JMeter plugin).** Most mature schema-aware option. This note.
- **Apache Kafka's `kafka-producer-perf-test.sh`** / `kafka-consumer-perf-test.sh`. Ships with Kafka; no schema awareness; max-throughput benchmark. First stop for protocol-level performance but not useful for realism.
- **Artillery with custom engine.** Artillery supports Kafka via community extensions but the UX is less polished.
- **k6 with xk6-kafka.** A k6 extension that enables Kafka producer/consumer testing. Younger than kloadgen but has k6's superior scenario machinery. Combines open-loop executors with Kafka protocol.
- **ShadowReader for Kafka.** Not directly; ShadowReader is HTTP-focused.
- **Custom Go programs.** The "real" path most teams take when kloadgen doesn't scale.

## Toolmaker gap revisited

The specific open-source white space: **AsyncAPI → Kafka load test generator with per-field distribution control, causal event chain modelling, and open-loop arrival rate.** None of the existing tools covers all three. kloadgen is the closest on the schema axis; xk6-kafka is the closest on the scenario axis; nobody does causal chains.

## Citations

- https://github.com/corunet/kloadgen
- AsyncAPI tools registry (references kloadgen): https://www.asyncapi.com/tools
- Confluent Schema Registry: https://docs.confluent.io/platform/current/schema-registry/index.html
- xk6-kafka: https://github.com/mostafa/xk6-kafka
- Kafka perf scripts: https://kafka.apache.org/documentation/#quickstart