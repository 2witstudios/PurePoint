# Code Review Rules

Imperative instructions for conducting code reviews on PurePoint.

## Review Process

Follow these steps in order. For each step, show your work.

1. **Analyze code structure and organization** — Is the code in the right place? Does it follow existing patterns? Are there orphaned files?
2. **Check adherence to coding standards** — Does it follow the language-specific conventions? Are naming patterns consistent?
3. **Evaluate test coverage and quality** — Use TDD/Rubric to score the tests. Are requirements covered? Are edge cases tested?
4. **Assess performance** — Are there obvious performance issues? N+1 queries, unnecessary allocations, blocking async operations?
5. **Deep scan for security vulnerabilities** — Explicitly list each OWASP Top 10 category and check for violations. Look for visible keys, hardcoded secrets, injection vectors.
6. **Validate architectural patterns** — Does it respect module boundaries? Are dependencies flowing in the right direction?
7. **Check documentation quality** — Are public APIs documented? Are docblocks minimal and useful? Do comments explain WHY, not WHAT?
8. **Verify requirements adherence** — Compare completed work to functional requirements. Compare to the task plan. Are all requirements met?
9. **Provide actionable feedback** — Specific improvements with file paths and line numbers.

## Constraints

- **Don't make changes** — Review only. Output serves as input for planning.
- **Avoid unfounded assumptions** — If unsure, note it and ask in the review response.
- Use docblocks for public APIs — but keep them minimal.
- Ensure there are no unused stray files or dead code.

## OWASP Top 10 Checklist (2021)

For every review, explicitly consider:
1. Broken Access Control
2. Cryptographic Failures
3. Injection (SQL, command, LDAP, XSS)
4. Insecure Design
5. Security Misconfiguration
6. Vulnerable and Outdated Components
7. Identification and Authentication Failures
8. Software and Data Integrity Failures
9. Security Logging and Monitoring Failures
10. Server-Side Request Forgery (SSRF)

## Security-Specific Rules

- Secret/token comparisons: require timing-safe comparison
- Authentication code: recommend opaque tokens over JWT
- All user input: validate and sanitize at system boundaries
- API keys and secrets: must never appear in source code
