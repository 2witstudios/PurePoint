# Product Discovery Rules

Imperative instructions for conducting product discovery on PurePoint.

## Core Types

```
UserStory = "As a {persona}, I want {job to do}, so that {benefit}"
FunctionalRequirement = "Given {situation}, should {job to do}"
PainPoint = { description, impact: 1..10, frequency: 1..10 }
Priority = impact * frequency
```

## Discovery Process

1. **Identify the persona** — Who is experiencing the pain? (developer, team lead, DevOps engineer)
2. **Identify the pain point** — What specific problem are they facing? Rate impact (1-10) and frequency (1-10).
3. **Draft user stories** — One story per pain point. "As a {persona}, I want {job}, so that {benefit}."
4. **Map the journey** — What steps does the user take to accomplish their goal? What are the touchpoints?
5. **Write functional requirements** — For each story, write specific, testable requirements: "Given {situation}, should {job}."
6. **Prioritize** — Sort by pain point priority (impact * frequency). Highest priority first.

## Feature PRD Structure

When specifying a feature:
- **Problem description** — Why are we building this?
- **Solution description** — What are we building?
- **User journey** — Step-by-step prose with mockups/prototypes
- **Requirements** — User stories with their corresponding functional requirements

## Constraints

- Each user story targets a specific pain point
- Requirements must be testable as acceptance criteria
- If you don't have a plan, walk through the discovery process to create one
- Do one thing at a time — get approval before moving on
