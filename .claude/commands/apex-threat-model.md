# APEX Threat Model Wizard

Interactive wizard that analyzes a project and generates a `[threat_model]` section for `apex.toml`.

## Usage
```
/apex-threat-model [target]
```
Examples:
- `/apex-threat-model` — analyze current directory
- `/apex-threat-model /path/to/project`

## Instructions

Parse `$ARGUMENTS`: target path. Default: `.`

### Step 1: Detect project type

Scan the target directory for signals:

**Web service signals** (check first — most specific):
```bash
# Python web frameworks
grep -rl "from flask\|from django\|from fastapi\|from starlette\|from sanic\|from tornado" <TARGET> --include="*.py" -l 2>/dev/null | head -3
# JS/TS web frameworks
grep -rl "express\|fastify\|koa\|hapi\|next\|nuxt" <TARGET>/package.json 2>/dev/null
# Java web
grep -rl "spring-boot\|javax.servlet\|jakarta.servlet\|dropwizard" <TARGET> --include="*.java" --include="*.xml" --include="*.gradle" -l 2>/dev/null | head -3
# Ruby web
grep -rl "rails\|sinatra\|rack" <TARGET>/Gemfile 2>/dev/null
```

**CLI tool signals**:
```bash
# Python CLI
grep -rl "argparse\|click\|typer\|fire\|docopt\|sys.argv" <TARGET> --include="*.py" -l 2>/dev/null | head -3
# Rust CLI
grep -rl "clap\|structopt\|argh" <TARGET>/Cargo.toml 2>/dev/null
# Go CLI
grep -rl "cobra\|urfave/cli\|flag\." <TARGET> --include="*.go" -l 2>/dev/null | head -3
```

**CI pipeline signals**:
```bash
ls <TARGET>/.github/workflows/*.yml <TARGET>/.gitlab-ci.yml <TARGET>/Jenkinsfile <TARGET>/.circleci/config.yml 2>/dev/null
grep -rl "CI=\|GITHUB_ACTIONS\|GITLAB_CI\|JENKINS" <TARGET> --include="*.py" --include="*.sh" -l 2>/dev/null | head -3
```

**Library signals** (default if nothing else matches):
```bash
# Check for lib-like project structure
ls <TARGET>/setup.py <TARGET>/pyproject.toml 2>/dev/null  # Python lib
grep "\\[lib\\]" <TARGET>/Cargo.toml 2>/dev/null         # Rust lib
```

### Step 2: Present detection results

Show what was detected and ask the user to confirm:

```
## Threat Model Detection

Project: <target>
Detected type: **<type>** (based on: <signals found>)

Available types:
1. **cli-tool** — Command-line application (argv, stdin, env vars are trusted)
2. **web-service** — HTTP server (request data is untrusted)
3. **library** — Reusable library (all inputs untrusted — strictest)
4. **ci-pipeline** — CI/CD automation (argv, env vars, files are trusted)

Is this correct? Enter a number to change, or press enter to accept.
```

Wait for user response. If they provide a number, use that type.

### Step 3: Ask about custom trust overrides

```
Do you have any custom trust overrides?

For example, if your web service receives requests only from internal services
(not public internet), you could mark "request" as trusted.

- Type source names to add as **trusted** (comma-separated), or press enter to skip.
```

Then:
```
- Type source names to add as **untrusted** (comma-separated), or press enter to skip.
```

### Step 4: Generate and show the config

Build the TOML section:

```toml
[threat_model]
type = "<detected-type>"
# trusted_sources = ["internal_api"]    # uncomment to override defaults
# untrusted_sources = ["legacy_input"]  # uncomment to override defaults
```

If custom overrides were provided:
```toml
[threat_model]
type = "<detected-type>"
trusted_sources = ["source1", "source2"]
untrusted_sources = ["source3"]
```

### Step 5: Show impact preview

Show what the threat model would suppress:

```
## Impact Preview

With type = "<type>", APEX will:
- **Suppress** findings where input flows from: <trusted sources for this type>
- **Keep** findings where input flows from: <untrusted sources for this type>

This typically reduces false positives by 60-90% for <type> projects.
```

Trust tables per type:

**cli-tool**: Trusted = argv, args, arg(, input, stdin, environ, getenv, file, format!, &str. Untrusted = recv, socket.
**web-service**: Trusted = environ, getenv, format!, &str. Untrusted = request, query, form, param, input, stdin, upload, user, recv, socket, file.
**library**: All untrusted (strictest — any input could come from anywhere).
**ci-pipeline**: Trusted = argv, args, arg(, stdin, environ, getenv, file, format!, &str. Untrusted = recv, socket.

### Step 6: Write to apex.toml

Check if `<TARGET>/apex.toml` exists:

```bash
ls <TARGET>/apex.toml 2>/dev/null
```

- If it exists: read it, check if `[threat_model]` section already exists.
  - If section exists: ask user if they want to replace it.
  - If no section: append the new section.
- If it doesn't exist: create a new `apex.toml` with just the `[threat_model]` section.

Ask for confirmation before writing:
```
Write this to <TARGET>/apex.toml? (y/n)
```

### Step 7: Suggest next steps

```
Threat model configured! Next steps:
  /apex-run   — re-run audit with threat model (should see fewer false positives)
  /apex       — full dashboard with updated metrics
```
