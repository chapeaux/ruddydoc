# @chapeaux/ruddydoc

Fast document conversion with embedded knowledge graph.

RuddyDoc is a Rust rewrite of [docling](https://github.com/docling-project/docling) with an embedded Oxigraph RDF datastore for SPARQL-queryable document knowledge graphs.

## Installation

```bash
npm install @chapeaux/ruddydoc
# or
npx @chapeaux/ruddydoc
```

This package downloads the appropriate pre-built binary for your platform during installation.

## Usage

```bash
# Convert a document
npx ruddydoc convert input.pdf --format json

# Get help
npx ruddydoc --help
```

## Supported Platforms

- macOS (x64, ARM64)
- Linux (x64, ARM64)
- Windows (x64)

## Manual Installation

If the binary download fails (e.g., behind a corporate firewall), you can install via Cargo:

```bash
cargo install ruddydoc
```

## License

MIT
