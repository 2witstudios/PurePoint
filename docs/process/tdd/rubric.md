# TDD Rubric

Scoring criteria for evaluating test quality during code review.

## Scoring (1-5 per criterion)

### 1. Five Questions Coverage (weight: 3x)
- 5: Every test clearly answers all 5 questions via assert format
- 4: Most tests answer all 5; minor gaps in given/should clarity
- 3: Tests have assertions but given/should are vague or describe literals
- 2: Tests exist but don't use structured assert; hard to understand intent
- 1: Tests are present but don't answer basic questions about behavior

### 2. Test Isolation (weight: 2x)
- 5: Zero shared mutable state; each test is completely independent
- 4: Mostly isolated; factory functions used; minor shared setup
- 3: Some shared fixtures but no inter-test dependencies
- 2: Shared mutable state exists between tests
- 1: Tests depend on execution order or external state

### 3. Requirement Coverage (weight: 3x)
- 5: Every functional requirement has corresponding test(s); edge cases covered
- 4: All core requirements tested; most edge cases covered
- 3: Core happy-path requirements tested; some edge cases missing
- 2: Partial coverage; significant requirements untested
- 1: Minimal or no meaningful coverage of requirements

### 4. Test Readability (weight: 2x)
- 5: Tests read as documentation; a new developer understands behavior from tests alone
- 4: Tests are clear; minor improvements possible in naming/structure
- 3: Tests are understandable but require some code reading to grasp intent
- 2: Tests are hard to follow; poor naming or structure
- 1: Tests are opaque; no clear connection between test name and behavior

### 5. No Redundancy (weight: 1x)
- 5: No type-shape tests, no framework-behavior tests, no duplicate tests
- 4: Minor redundancy that doesn't obscure the suite
- 3: Some unnecessary tests that add noise
- 2: Significant redundancy obscuring the meaningful tests
- 1: More noise than signal in the test suite

## Score Calculation

Weighted total / max possible = percentage
- 90-100%: Exemplary — publish as reference
- 75-89%: Strong — minor improvements only
- 60-74%: Adequate — address specific gaps
- 40-59%: Needs Work — significant revision required
- Below 40%: Insufficient — rewrite tests
