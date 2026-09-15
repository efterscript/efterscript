## ADDED Requirements

### Requirement: uncoloured-cell-colour-operators

Inside the paint procedure of an uncoloured tiling pattern the colour
operators SHALL raise `undefined`, per PLRM3 §4.9.2. The reference
lets such a procedure set a colour and paints with it. Chosen because
the manual states the error and an uncoloured cell carries no colour
of its own in the PDF. No configuration restores the lenient
behaviour.

#### Scenario: A colour set inside an uncoloured cell

- **GIVEN** an uncoloured pattern whose paint procedure calls `setgray`
- **THEN** the first paint with it raises `undefined`, where the
  reference paints

### Requirement: capture-refuses-page-operators

Page operators (`showpage`, `copypage`, `erasepage`, the page-device
operators) inside a pattern paint procedure or a form procedure SHALL
raise `undefined`. The reference executes them. Chosen because the
procedure's marks are being captured into a reusable resource, which
has no page to end; the manual does not define the behaviour. No
configuration restores it.

#### Scenario: showpage inside a form

- **GIVEN** a form whose procedure calls `showpage`
- **THEN** `execform` raises `undefined`, where the reference emits a
  page

### Requirement: resource-instance-shape

`defineresource` in the `Pattern` and `Form` categories SHALL check the
instance's shape (the required keys with their types and ranges) and
raise `typecheck` or `rangecheck` for a wrong one. The reference
accepts any dictionary. Chosen so a malformed resource fails where it
is defined rather than where it is used; no configuration restores the
lenient behaviour.

#### Scenario: A dictionary without PaintProc

- **GIVEN** `<< /FormType 1 >> /X /Form defineresource`
- **THEN** the error is `typecheck`, where the reference defines it
