---
id: 01KNZ5QA24034HTXECJAA7EFY7
title: AWS Distributed Load Testing — CloudFormation reference stack with Fargate and Taurus
type: literature
tags: [tool, aws, distributed-load-testing, saas, fargate, taurus, self-hosted, cloudformation]
links:
  - target: 01KNZ56MS27NFFMFBHZ0WPCV70
    type: related
  - target: 01KNZ5F8S3YZFEGJX006A5WRA5
    type: related
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:03:49.316731+00:00
modified: 2026-04-11T21:03:49.316733+00:00
---

Source: https://docs.aws.amazon.com/solutions/latest/distributed-load-testing-on-aws/ — AWS Solutions Library implementation guide, fetched 2026-04-12.

"Distributed Load Testing on AWS" is a reference implementation (not a managed service) provided by the AWS Solutions Library. It is a CloudFormation-deployed stack that provisions a containerised load-testing workflow you own, configure, and pay for at AWS infrastructure rates. It is therefore cheaper than SaaS competitors at scale and more work to operate.

## Architecture

The solution deploys:
- **Amazon ECS on AWS Fargate** as the test runner. Each test invocation launches one or more Fargate tasks running a Taurus container (Taurus is BlazeMeter's OSS YAML wrapper). Tasks run for the duration of the test, then exit.
- **Multi-region deployment** — regional stacks can be spawned to generate load from multiple AWS regions simultaneously.
- **S3** for storing test scenarios, Taurus configs, and result files.
- **DynamoDB** for test metadata and scheduling state.
- **API Gateway + Lambda** as the control plane.
- **CloudFront + S3-hosted web UI** as the React frontend.
- **Amazon Cognito** for authentication.
- **CloudWatch** for test runner metrics and logs.

## Supported test frameworks

Via Taurus: **JMeter, k6, Locust**, plus a simple URL-based HTTP mode for ad-hoc targets. JMeter is the first-class path; the others are Taurus-backend routes.

## Capabilities

- Simulate tens of thousands of concurrent users per test.
- Multi-region distributed test execution (each region runs its own ECS tasks).
- Schedule tests for immediate, future, or recurring execution.
- Run multiple concurrent tests across different scenarios and regions simultaneously.
- Download raw results from S3 for offline analysis.

## Web UI

A React app hosted on CloudFront lets users upload test scripts, configure parameters, start/schedule runs, and view results. Cognito handles login. The UI is functional but not as polished as BlazeMeter or Grafana Cloud k6.

## Cost model

Advertised estimate: ~$30.90/month for idle AWS resources in us-east-1 (API Gateway, Lambda, DynamoDB, CloudFront, S3). On top of that, each test run costs whatever the Fargate tasks + data transfer cost — a 30-minute test with 10 Fargate tasks runs around $1–$5 depending on task size.

The core pricing advantage over SaaS: no per-VUH markup. The disadvantage: you pay for Fargate, operations, and upgrades yourself.

## Strengths

- Open-source reference architecture (CloudFormation templates are on GitHub).
- Native AWS IAM, VPC, and network access — reach private endpoints without VNet gymnastics.
- Multi-region orchestration is built in.
- Cheaper than BlazeMeter/Grafana Cloud k6 at steady use.
- Runs your existing Taurus/JMeter/k6/Locust scripts with no rewrite.

## Failure modes

- **Not a managed service** — you own the CloudFormation stack, IAM policies, Cognito config, and region sprawl.
- **Upgrading the solution** requires redeploying the stack; state migration is not automatic.
- **Fewer built-in analytics** than SaaS alternatives — result files are in S3 as Taurus output, so a Grafana/Athena/QuickSight setup is usually needed to make them useful.
- **Taurus-intermediated** — same abstraction-leakage issues as BlazeMeter's Taurus layer.
- **Solution lags BlazeMeter** on newer framework features.

## Relevance to APEX G-46

AWS Distributed Load Testing represents the "DIY SaaS" end of the landscape: same architecture as a commercial service, but open-source, self-operated, and customizable. For APEX, it is the natural target when "generate a reproduction script for a G-46 finding and run it across five AWS regions for 24 hours" is the workflow. The Taurus intermediate format is also where APEX's workload output would hook in most naturally.