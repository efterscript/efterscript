# expected-divergences

## MODIFIED Requirements

### Requirement: job-server-save-level

`vmstatus` SHALL report save level 0 for a program run directly, outside
a job server, where an interpreter serving jobs reports 1 for the job's
encapsulating save; a job served by a printer reports 1 as well. The
corpus runs programs directly.

#### Scenario: Declared

- **GIVEN** the corpus file printing the save level
- **THEN** it carries `% divergence: job-server-save-level`
