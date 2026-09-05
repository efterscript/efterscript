# interpreter-core

## ADDED Requirements

### Requirement: Execution budget

The interpreter's limits SHALL include an optional budget of executed
objects; when set and exceeded, execution SHALL stop with `limitcheck`
attributed to the object being executed, and the job SHALL report that
outcome. Unset, execution is unbounded as before.

#### Scenario: Budget exceeded

- **GIVEN** an interpreter with a budget of 10000 and the program
  `{ } loop`
- **THEN** the outcome is the `limitcheck` error and the interpreter
  returns
