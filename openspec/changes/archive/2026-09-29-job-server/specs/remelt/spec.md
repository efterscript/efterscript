# remelt

## ADDED Requirements

### Requirement: A document over a lent interpreter

A distillation SHALL be constructible over an existing interpreter and a
sink, installing a fresh graphics backend over the sink and seeding the
sink's parameters into the interpreter, and SHALL be finishable so that
it returns the report, the writer, and the interpreter with its graphics
backend detached. Fonts, patterns, forms, and colour spaces defined in
the interpreter before the document began SHALL be written into the
document when it uses them, as if defined within it.

#### Scenario: Two documents from one interpreter

- **GIVEN** an interpreter lent to a distillation that shows one page and is finished keeping the interpreter, then lent to a second that shows two
- **THEN** the first document has one page, the second has two, and each is a complete PDF
