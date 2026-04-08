#!/bin/bash
#
# RuddyDoc vs Python Docling performance comparison.
#
# Generates benchmark fixtures, runs RuddyDoc benchmarks, and optionally
# compares against Python docling if it is installed.
#
# Usage:
#   ./scripts/bench_vs_docling.sh           Run full comparison
#   ./scripts/bench_vs_docling.sh --rust    Run only RuddyDoc benchmarks
#   ./scripts/bench_vs_docling.sh --gen     Generate fixtures only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_DIR="${PROJECT_DIR}/tests/fixtures"

# Fixture sizes
SMALL_LINES=100
MEDIUM_LINES=1000
LARGE_LINES=10000

# ---------------------------------------------------------------------------
# Generate benchmark fixtures using a quick Rust helper
# ---------------------------------------------------------------------------
generate_fixtures() {
    echo "=== Generating benchmark fixtures ==="

    # Build the bench crate to verify it compiles
    cargo build --release -p ruddydoc-bench 2>/dev/null

    # Generate fixtures using a tiny Rust program via cargo test
    # (The fixtures are generated programmatically by the bench crate's lib)
    cat > /tmp/ruddydoc_gen_fixtures.rs << 'RUSTEOF'
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixture_dir = &args[1];

    // We cannot import ruddydoc_bench directly from a standalone script,
    // so we replicate the generation inline. The actual benchmark uses
    // the library's generate_large_markdown().
    println!("Fixture directory: {fixture_dir}");
    println!("Note: Fixtures are generated inline by the benchmark harness.");
    println!("Static fixture files (sample.md, etc.) are used for baseline benchmarks.");
}
RUSTEOF

    echo "  Fixtures will be generated dynamically by the benchmark harness."
    echo "  Static fixtures in ${FIXTURE_DIR}/ are used for baseline benchmarks."
    echo ""
}

# ---------------------------------------------------------------------------
# Run RuddyDoc benchmarks
# ---------------------------------------------------------------------------
run_rust_benchmarks() {
    echo "=== RuddyDoc Criterion Benchmarks ==="
    echo ""

    echo "--- Parsing benchmarks ---"
    cargo bench --bench parsing -p ruddydoc-bench 2>&1 | tail -n +2
    echo ""

    echo "--- Export benchmarks ---"
    cargo bench --bench export -p ruddydoc-bench 2>&1 | tail -n +2
    echo ""

    echo "--- Graph benchmarks ---"
    cargo bench --bench graph -p ruddydoc-bench 2>&1 | tail -n +2
    echo ""

    echo "Criterion HTML reports: target/criterion/report/index.html"
}

# ---------------------------------------------------------------------------
# Run Python docling benchmarks (if installed)
# ---------------------------------------------------------------------------
run_python_benchmarks() {
    if ! python3 -c "import docling" 2>/dev/null; then
        echo "=== Python docling not installed, skipping comparison ==="
        echo "  Install with: pip install docling"
        return
    fi

    echo "=== Python Docling Benchmarks ==="
    echo ""

    # Startup time
    echo "--- Startup time ---"
    echo -n "  RuddyDoc:  "
    if [ -x "${PROJECT_DIR}/target/release/ruddydoc" ]; then
        /usr/bin/time -f "%e seconds" "${PROJECT_DIR}/target/release/ruddydoc" --version 2>&1 || true
    else
        echo "(binary not built; run: cargo build --release -p ruddydoc-cli)"
    fi

    echo -n "  Docling:   "
    /usr/bin/time -f "%e seconds" python3 -c "import docling" 2>&1 || true
    echo ""

    # Markdown conversion
    echo "--- Convert sample.md to JSON ---"
    echo -n "  RuddyDoc:  "
    if [ -x "${PROJECT_DIR}/target/release/ruddydoc" ]; then
        /usr/bin/time -f "%e seconds" "${PROJECT_DIR}/target/release/ruddydoc" convert "${FIXTURE_DIR}/sample.md" > /dev/null 2>&1 || true
    else
        echo "(binary not built)"
    fi

    echo -n "  Docling:   "
    /usr/bin/time -f "%e seconds" python3 -c "
from docling.document_converter import DocumentConverter
converter = DocumentConverter()
result = converter.convert('${FIXTURE_DIR}/sample.md')
result.document.export_to_dict()
" 2>&1 || echo "(failed)"
    echo ""

    # HTML conversion
    echo "--- Convert sample.html to JSON ---"
    echo -n "  RuddyDoc:  "
    if [ -x "${PROJECT_DIR}/target/release/ruddydoc" ]; then
        /usr/bin/time -f "%e seconds" "${PROJECT_DIR}/target/release/ruddydoc" convert "${FIXTURE_DIR}/sample.html" > /dev/null 2>&1 || true
    else
        echo "(binary not built)"
    fi

    echo -n "  Docling:   "
    /usr/bin/time -f "%e seconds" python3 -c "
from docling.document_converter import DocumentConverter
converter = DocumentConverter()
result = converter.convert('${FIXTURE_DIR}/sample.html')
result.document.export_to_dict()
" 2>&1 || echo "(failed)"
    echo ""

    # CSV conversion
    echo "--- Convert sample.csv to JSON ---"
    echo -n "  RuddyDoc:  "
    if [ -x "${PROJECT_DIR}/target/release/ruddydoc" ]; then
        /usr/bin/time -f "%e seconds" "${PROJECT_DIR}/target/release/ruddydoc" convert "${FIXTURE_DIR}/sample.csv" > /dev/null 2>&1 || true
    else
        echo "(binary not built)"
    fi

    echo -n "  Docling:   "
    /usr/bin/time -f "%e seconds" python3 -c "
from docling.document_converter import DocumentConverter
converter = DocumentConverter()
result = converter.convert('${FIXTURE_DIR}/sample.csv')
result.document.export_to_dict()
" 2>&1 || echo "(failed)"
    echo ""
}

# ---------------------------------------------------------------------------
# Print summary
# ---------------------------------------------------------------------------
print_summary() {
    echo "=== Benchmark Summary ==="
    echo ""
    echo "RuddyDoc Criterion results are in: target/criterion/"
    echo "Open target/criterion/report/index.html in a browser for HTML reports."
    echo ""
    echo "Key benchmarks to watch:"
    echo "  - parse_markdown_scaling/lines/1000   (target: <1ms)"
    echo "  - parse_markdown_scaling/lines/10000  (target: <10ms)"
    echo "  - export_json_500_lines               (target: <5ms)"
    echo "  - sparql_ordered_1000_elements         (target: <10ms)"
    echo "  - e2e_markdown_to_json/lines/1000     (target: <5ms)"
    echo "  - chunk_500_lines_default              (target: <5ms)"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
cd "${PROJECT_DIR}"

case "${1:-}" in
    --gen)
        generate_fixtures
        ;;
    --rust)
        run_rust_benchmarks
        print_summary
        ;;
    *)
        generate_fixtures
        run_rust_benchmarks
        run_python_benchmarks
        print_summary
        ;;
esac
