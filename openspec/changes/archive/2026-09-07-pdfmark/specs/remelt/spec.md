# remelt

## ADDED Requirements

### Requirement: Document objects from marks

The output SHALL contain, from the IR's marks: an `/Outlines` tree
whose items follow the mark order with the count-and-sign nesting rule
and reference their destinations; named destinations in the catalog;
per-page `/Annots` with link annotations (`/Dest` by name or a URI
action, border and colour when given); `/Info` entries from document
information alongside the producer; `/PageMode`, `/PageLayout`, and
`/OpenAction`; and `CropBox` and `Rotate` on pages. Output SHALL stay
deterministic and a document without marks SHALL be byte-identical to
today's.

#### Scenario: Bookmarks and links in the PDF

- **GIVEN** the nested-bookmark and cross-page link scenarios
- **THEN** the document's catalog references an outlines tree with
  three items, page 2 has one link annotation whose destination
  resolves to page 1, and a checker accepts the file

#### Scenario: No marks, no change

- **GIVEN** the stroked-line corpus file
- **THEN** its PDF golden is byte-identical
