## ADDED Requirements

### Requirement: shading-colour-conversion-limit

A shading whose `ColorSpace` is a CIE-based space that does not
collapse to a calibrated space SHALL raise `limitcheck`. The reference
paints it. Chosen because the shading's colours come from a function
the viewer evaluates, so converting them to L*a*b* would require
evaluating the function in the interpreter; no configuration restores
the reference's behaviour.

#### Scenario: Declared

- **GIVEN** the corpus file painting an axial shading in a
  non-collapsing CIE-based space
- **THEN** it carries `% divergence: shading-colour-conversion-limit`

### Requirement: sampled-function-short-source

A type 0 function whose `DataSource` holds fewer bytes than `Size`,
`BitsPerSample`, and the output count imply SHALL raise `rangecheck`
when the shading using it is read. The reference paints, taking the
missing samples as zero. Chosen because the function is carried as a
value the viewer evaluates, and a viewer given a short sample stream
may refuse it or read past it; the manual gives no default for the
missing samples. No configuration restores the lenient behaviour.

#### Scenario: Declared

- **GIVEN** the corpus file whose sampled function declares four
  samples over three bytes
- **THEN** it carries `% divergence: sampled-function-short-source`

### Requirement: incomplete-mesh-element

Mesh data that ends inside a vertex, a triangle begun by a flag of 0,
a lattice row, or a patch SHALL raise `rangecheck`. The reference
paints the whole elements and drops the remainder. Chosen because the
data is written to the PDF as it was checked, and a viewer given a
partial element may refuse the whole shading; the manual requires the
elements to be complete. No configuration restores the lenient
behaviour.

#### Scenario: Declared

- **GIVEN** the corpus file whose triangle mesh ends after two vertices
  of a new triangle
- **THEN** it carries `% divergence: incomplete-mesh-element`
