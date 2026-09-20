## 1. Workflow

- [x] 1.1 Split `artefacts` into per-platform build jobs (x86_64 host + wasm on `ubuntu-latest`; aarch64 host on `ubuntu-24.04-arm`) that upload workflow artifacts, each with its native link-and-run check
- [x] 1.2 One `attach` job downloads them, writes `SHA256SUMS`, and attaches to the release

## 2. Docs

- [x] 2.1 The embedding guide's "Prebuilt archives" lists both host triples
