## ADDED Requirements

### Requirement: Shadings are resources

The IR SHALL carry shadings as page resources holding the shading
type, colour space, background, bounding box, the type's geometry,
its function (a value: sampled with its samples, exponential, or
stitching), and for mesh types the packed vertex data with its bit
depths and decode ranges; a pattern resource SHALL be either a tiling
cell or a shading with its matrix; a page SHALL carry shade operations
referencing a shading with the matrix from the shading's space to
default user space. The dump SHALL print each shading with its
entries and its function, and each shade operation; pages without
them SHALL dump exactly as before.

#### Scenario: An axial shading in the dump

- **WHEN** a page paints an axial shading with `shfill`
- **THEN** the dump lists one shading resource with its type, coordinates, extension, and function, and a shade line with the matrix

#### Scenario: A shading pattern in the dump

- **WHEN** a page fills with a shading pattern
- **THEN** the dump's pattern line names the shading and its matrix and the fill's colour line names the pattern
