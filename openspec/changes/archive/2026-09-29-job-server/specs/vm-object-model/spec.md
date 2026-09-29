# vm-object-model

## MODIFIED Requirements

### Requirement: Save and restore

`save` SHALL snapshot the local VM in constant time. `restore` SHALL revert
every local composite object to its value at the corresponding `save`,
invalidate every local composite object and save object created after it,
and raise `invalidrestore` if any stack still references such an object.
Global VM SHALL be unaffected by `restore`, except at the outermost save
level of a job server (see `job-server`): the job server's own save, and
a save executed by an unencapsulated job with no save pending, SHALL
snapshot global VM as well, also in constant time, and their `restore`
SHALL revert it.

#### Scenario: Local values revert

- **GIVEN** `/a [1 2 3] def save`
- **WHEN** `a 0 9 put restore` is executed
- **THEN** `a 0 get` is 1

#### Scenario: Objects created after save are rejected on the stack

- **GIVEN** `save` followed by `[1 2]` left on the operand stack
- **WHEN** `restore` is executed with that array still on the stack
- **THEN** `invalidrestore` is raised

#### Scenario: Global values persist

- **GIVEN** `true setglobal /g [1] def false setglobal save`
- **WHEN** `g 0 2 put restore` is executed
- **THEN** `g 0 get` is 2

#### Scenario: The outermost save of an unencapsulated job

- **GIVEN** a job server running `true 0 startjob pop true setglobal globaldict /g [1] put false setglobal save`
- **WHEN** `globaldict /g get 0 2 put restore globaldict /g get 0 get =` is executed
- **THEN** the output is `1`
