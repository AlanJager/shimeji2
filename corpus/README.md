# Optional local pack corpus

Third-party packs, ZIPs, extracted artwork and generated traces stay local and are
not part of this repository. Corpus-dependent tests are ignored unless their data
is explicitly provided. Ordinary tests use the bundled shime and synthetic fixtures.

If maintaining a local corpus, record the pack sources, licensing notes and hashes
in a local manifest.toml. Do not publish pack art without a suitable distribution
license. The bundled upstream shime and its notices live in assets/packs/shime/.
