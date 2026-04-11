---
id: 01KNZ2ZDM1XWWE840ADNCGKBMP
title: "Joel Spolsky: Back to Basics (Schlemiel the Painter)"
type: literature
tags: [strcat, schlemiel, quadratic, c, strings, joelonsoftware, performance]
links:
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.joelonsoftware.com/2001/12/11/back-to-basics/"
---

# Joel Spolsky — "Back to Basics" (Dec 11, 2001)

*Source: https://www.joelonsoftware.com/2001/12/11/back-to-basics/ — fetched 2026-04-12. The founding essay of the "Schlemiel the Painter" meme in programming performance folklore.*

## The parable

Spolsky tells an old Yiddish joke: "Shlemiel gets a job as a street painter, painting the dotted lines down the middle of the road. On the first day he takes a can of paint out to the road and finishes 300 yards of the road. 'That's pretty good!' says his boss, 'you're a fast worker!' and pays him a kopeck. The next day Shlemiel only gets 150 yards done. 'Well, that's not nearly as good as yesterday, but you're still a fast worker. 150 yards is respectable,' and pays him a kopeck. The next day Shlemiel paints 30 yards of the road. 'Only 30!' shouts his boss. 'That's unacceptable! On the first day you did ten times that much work! What's going on?' 'I can't help it,' says Shlemiel. 'Every day I get farther and farther away from the paint can!'"

Shlemiel is `strcat` in a loop. Each call walks to the end of the destination string before appending — the walk is proportional to the length of what has already been written, so appending `n` equal-size chunks costs `O(n²)` character copies.

## The C-string root cause

C strings are NUL-terminated byte sequences. Length is not stored anywhere; every operation that needs the length must scan from the start. The standard `strcat` signature is:

```c
char *strcat(char *dest, const char *src);
```

Implementation (canonical, paraphrased from Spolsky's illustration):

```c
char *strcat(char *dest, const char *src) {
    while (*dest) dest++;           // walk to end: O(|dest|)
    while ((*dest++ = *src++));     // copy:        O(|src|)
    return dest_original;           // which we didn't save...
}
```

Calling `strcat` `n` times in a loop to build an `n·k`-character string takes:
```
k + 2k + 3k + ... + nk = O(n² · k) character copies.
```

## Pascal strings — the alternative

Spolsky contrasts with Pascal strings, which store length as the first byte. `strcat` on Pascal strings is `O(|src|)` because you can index straight to the end. The downside is the 255-byte limit of the one-byte length prefix, or the memory cost of using 4 bytes for length on 32-bit systems. Modern languages (Java, Python, Go, Rust) all use length-prefixed strings internally precisely because `O(1)` length access enables `O(n)` concatenation.

## The "return pointer to end" fix

Spolsky's suggested fix is `mystrcat` — a variant of strcat that returns a pointer to the new end of the destination:

```c
char *mystrcat(char *dest, const char *src) {
    while (*dest) dest++;
    while ((*dest++ = *src++));
    return --dest;
}
```

Now a loop can thread the end-pointer forward:

```c
char *p = buffer;
*p = '\0';
for (int i = 0; i < n; i++) p = mystrcat(p, pieces[i]);
```

Total cost: `O(total output size)` — linear. The fix is trivial; the point of the essay is that the canonical C library shape of `strcat` is a performance booby trap **by API design**, and a whole generation of programmers walked into it.

## Real-world Schlemiel instances

- **Netscape 6 paint loop** — rumoured to have had O(n²) text layout from concatenating HTML DOM text into a single string per layout pass.
- **Moment.js early duration parsing** — regex split + string concat.
- **Python pre-CPython 2.4 `str +=` in a loop** — pre-refcount optimisation, O(n²). Post-2.4 CPython has a special-case in the bytecode interpreter that in-place extends when refcount == 1. Still not portable; PyPy and Jython retain the O(n²).
- **Java `String + String` in a loop** on pre-Java-9 VMs that didn't fold it into a `StringBuilder` at the bytecode level.
- **Go `s += t` in a loop** — always O(n²); `strings.Builder` is linear.

## Broader lesson: leaky abstractions

Spolsky's thesis generalises: "**high-level programming environments leak details of their low-level implementations through performance.**" A programmer who doesn't understand how strings are laid out in memory will build an application that feels snappy for a single user and collapses at `n = 10000`. This is the exact failure mode APEX G-46's empirical complexity estimator is designed to catch automatically — you don't need the user to know about Schlemiel, you just need the tool to run the function at `n = {10, 100, 1000, 10000}` and notice the slope.

## Relevance to APEX G-46

1. **Detector rule: `+=` inside a `for`/`while` loop on string-typed variables.** ruff `PLW3301`, pylint `R5501`, and Go's `gocritic` already flag this; APEX should too, with CWE-407 as the finding category.
2. **Detector rule: `strcat`/`strncat` inside a loop in C/C++.** There is almost never a reason to do this; the loop body should use `snprintf` with a running offset, or `memcpy` with a tracked end-pointer.
3. **Dynamic detector.** The quadratic is so well-defined that APEX's empirical complexity estimator will catch it reliably by running the target function at 10, 100, 1000, 10000 iterations and checking the exponent — the paradigm case for G-46's complexity-classification feature.
4. **The essay is the *why* to link in Findings** — when APEX flags an `accum += chunk` loop, the remediation pointer should be Spolsky's "Back to Basics". It is the clearest one-page explanation for a non-specialist.

## References

- Spolsky — "Back to Basics" — [joelonsoftware.com/2001/12/11/back-to-basics](https://www.joelonsoftware.com/2001/12/11/back-to-basics/)
- Related note: Quadratic String Accumulation (`01KNWE2QABV7943DKAXTARJHXA`)
- Pascal String — [en.wikipedia.org/wiki/String_(computer_science)#Length-prefixed](https://en.wikipedia.org/wiki/String_(computer_science))
