# interpreter-core

## ADDED Requirements

### Requirement: Trigonometric argument reduction

`sin` and `cos` SHALL reduce their argument modulo 360 degrees in
double precision before conversion to radians, so results for large
angles keep single precision's digits.

#### Scenario: A large angle

- **GIVEN** `1000000 cos =`
- **THEN** the output is `0.17364818` within one unit in the last place
