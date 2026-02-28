# Code Review Rubric

Scoring criteria for evaluating code quality during review.

## Scoring (1-5 per criterion)

### 1. Requirements Fidelity (weight: 3x)
- 5: All functional requirements met; acceptance criteria satisfied; no scope creep
- 4: Core requirements met; minor gaps in edge cases
- 3: Most requirements met; some acceptance criteria missing
- 2: Significant requirements unmet
- 1: Work does not address the stated requirements

### 2. Code Quality (weight: 3x)
- 5: Clean, idiomatic, minimal; no dead code; clear naming; single responsibility
- 4: Well-structured; minor improvements possible
- 3: Functional but could be simplified; some redundancy
- 2: Overly complex; significant dead code or redundancy
- 1: Hard to understand; needs major refactoring

### 3. Test Quality (weight: 2x)
- Defer to TDD/Rubric for detailed scoring
- Map TDD Rubric percentage to this 1-5 scale

### 4. Security (weight: 3x)
- 5: No OWASP violations; proper input validation; secrets handled correctly
- 4: No critical issues; minor hardening opportunities
- 3: Low-severity issues present; no exploitable vulnerabilities
- 2: Medium-severity vulnerabilities found
- 1: Critical security vulnerabilities present

### 5. Architecture (weight: 2x)
- 5: Respects module boundaries; dependencies flow correctly; extensible without modification
- 4: Sound architecture; minor coupling issues
- 3: Works but some architectural concerns
- 2: Violates module boundaries or creates tight coupling
- 1: Fundamentally misarchitected

### 6. Documentation (weight: 1x)
- 5: Public APIs documented; comments explain WHY; no stale docs
- 4: Well-documented; minor gaps
- 3: Adequate documentation
- 2: Under-documented public APIs
- 1: No meaningful documentation

## Score Calculation

Weighted total / max possible = percentage
- 90-100%: Ship it
- 75-89%: Minor revisions
- 60-74%: Revisions needed
- 40-59%: Significant rework
- Below 40%: Redesign required
