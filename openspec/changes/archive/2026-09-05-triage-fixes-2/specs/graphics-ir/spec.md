# graphics-ir

## MODIFIED Requirements

### Requirement: Page device tolerance

`setpagedevice` SHALL extract the media box from a page-size entry, record
all other entries retrievably via `currentpagedevice`, and never raise an
error for an unknown key. For the keys it recognises it SHALL raise
`typecheck` when the value has the wrong type: dictionaries for
`InputAttributes`, `OutputAttributes`, and `Policies`; booleans for
`Duplex`, `Collate`, and `Tumble`; integers for `NumCopies` and
`Orientation`; arrays or null for `ImagingBBox`, `HWResolution`, and
`PageOffset`.

#### Scenario: Unknown keys accepted

- **GIVEN** `<< /PageSize [612 792] /TraySwitch true >> setpagedevice`
- **THEN** no error is raised, the page media box is 612×792, and
  `currentpagedevice /TraySwitch get` is `true`

#### Scenario: Ill-typed known key

- **GIVEN** `<< /InputAttributes (tray) >> setpagedevice`
- **THEN** the error is `typecheck`
