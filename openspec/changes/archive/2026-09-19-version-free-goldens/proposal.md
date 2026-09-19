# version-free-goldens

## Why

The PDF sink writes the project's version into the Info `Producer`, and
the golden documents pin it: the bump to 0.0.2 broke 154 goldens and the
two tests that compare a sink's bytes against one, failing the tag's
verification and the release with it. A release must not rewrite every
golden.

## What changes

- The sink gains an option that names the project alone in `Producer`,
  for output whose bytes must not change with a release. The default is
  unchanged: the version stays in every document a user distils.
- The corpus runner writes goldens with that option; the two tests that
  compare bytes against a golden use it as well.
- The goldens are regenerated once, the only change being the `Producer`
  line and the offsets after it.

## Non-goals

Any other change to the goldens, the Info dictionary, or the producer's
wording.
