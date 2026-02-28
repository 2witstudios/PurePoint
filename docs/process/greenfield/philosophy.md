# Greenfield Philosophy

## Why Greenfield Matters

Starting a new module, crate, or component is the highest-leverage moment in a codebase. The patterns you set here become the conventions everything else follows. Get it right and the module grows naturally. Get it wrong and you'll fight the structure forever.

## Principles

### Start with the Interface, Not the Implementation
What will consumers of this module call? What will they pass in? What will they get back? Design the API first, then build behind it.

### One Module, One Job
A new crate or module should have a single, clear responsibility. If you can't describe it in one sentence, it's too big.

### Convention Over Configuration
Follow existing patterns in the codebase. If other crates use builder patterns, use builders. If other modules export a `new()` constructor, do the same. Consistency trumps local optimization.

### Test the Interface, Not the Guts
Write tests against the public API of the new module. Internal implementation should be free to change without breaking tests.
