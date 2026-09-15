## ADDED Requirements

### Requirement: Patterns and forms are page resources

The IR SHALL carry tiling patterns and forms as page resources
holding their own operation lists, captured in pattern space and form
space respectively with their bounding boxes, and (for patterns) the
paint type, tiling type, steps, and the matrix from pattern space to
default user space. Operations inside a resource SHALL reference the
same page-level colour spaces, images, fonts, patterns, and forms as
page operations. A fill, stroke, text run, or image mask SHALL be able
to carry a pattern as its colour, with the underlying components of an
uncoloured pattern; a page SHALL carry form placements with the matrix
from form space to default user space. The dump SHALL list each
pattern and form resource with its content and note a pattern colour,
and pages without them SHALL dump exactly as before.

#### Scenario: A pattern in the dump

- **WHEN** a page fills with a coloured pattern
- **THEN** the dump lists one pattern resource with its matrix, box, steps, and the captured fill, and the page's fill line names the pattern

#### Scenario: A form placement in the dump

- **WHEN** a page executes a form twice
- **THEN** the dump lists one form resource with its box and content and the page carries two placement lines with their matrices

#### Scenario: Capture nests

- **WHEN** a form's procedure fills with a pattern
- **THEN** the pattern resource belongs to the page and the fill inside the form resource names it
