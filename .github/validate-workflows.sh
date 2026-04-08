#!/bin/bash
# Validate GitHub Actions workflows
# Usage: .github/validate-workflows.sh

set -e

WORKFLOWS_DIR=".github/workflows"

echo "Validating GitHub Actions workflows..."

# Check if Python is available
if ! command -v python3 &> /dev/null; then
    echo "Warning: python3 not found, skipping YAML validation"
    exit 0
fi

# Install PyYAML if not already installed
if ! python3 -c "import yaml" &> /dev/null; then
    echo "Installing PyYAML for validation..."
    pip3 install --user PyYAML || {
        echo "Warning: Could not install PyYAML, skipping validation"
        exit 0
    }
fi

# Validate each workflow file
for workflow in "$WORKFLOWS_DIR"/*.yml; do
    if [ -f "$workflow" ]; then
        echo "Validating $workflow..."
        python3 -c "
import yaml
import sys
try:
    with open('$workflow', 'r') as f:
        yaml.safe_load(f)
    print('✓ $workflow is valid YAML')
except yaml.YAMLError as e:
    print('✗ $workflow has YAML errors:', e)
    sys.exit(1)
"
    fi
done

echo "All workflows validated successfully!"
