# APEX TODO

## JS/TS Concolic — Not Yet Covered

- [ ] Dynamic `eval()` / `new Function()` — not statically analyzable, needs runtime tracing
- [ ] Proxy/Reflect metaprogramming — intercepted property access creates invisible branches
- [ ] Async control flow constraints — Promise branching, `await` paths, race conditions
