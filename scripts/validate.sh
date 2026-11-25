#!/usr/bin/env bash
# Validate a signed PNG against off-the-shelf C2PA tools and record the results.
# Usage: scripts/validate.sh [signed.png]
# Defaults to out/signed.png (the demo output).
#
# Records results to docs/validation-report.md.
# Requires: c2patool on PATH (cargo install c2patool).
# C2P-32: "Output is run through at least one off-the-shelf C2PA validation tool
#          and the result is recorded."
# Assertion label: io.github.c2pa-thesis.zkloc (v6)

set -euo pipefail

signed="${1:-out/signed.png}"
report="docs/validation-report.md"
out_dir="out/validation"

if [[ ! -f "$signed" ]]; then
  echo "File not found: $signed"
  echo "Run 'provenance demo' first, or pass a signed PNG."
  exit 1
fi

if ! command -v c2patool &>/dev/null; then
  echo "c2patool not found. Install it:"
  echo "  cargo install c2patool"
  echo "  # or: c2pa/scripts/setup.sh"
  exit 1
fi

mkdir -p "$out_dir"

echo "=== C2P-32 validation ==="
echo "File: $signed"
echo "Size: $(wc -c < "$signed" | tr -d ' ') bytes"
echo

# 1. c2patool: read the manifest back
echo "--- c2patool manifest read ---"
c2patool "$signed" --detailed 2>&1 | tee "$out_dir/c2patool-detailed.json"
echo

# 2. Extract just the custom assertion
echo "--- Custom assertion extraction ---"
provenance inspect "$signed" > "$out_dir/assertion.json" 2>&1
echo "Assertion written to $out_dir/assertion.json"
echo

# 3. Verify with provenance (our reader)
echo "--- provenance verify ---"
provenance verify "$signed" --json 2>&1 | tee "$out_dir/verify.json"
echo

# 4. Collect c2patool version
c2patool_version=$(c2patool --version 2>&1 || echo "unknown")
provenance_version=$(provenance --version 2>&1 || echo "unknown")

# 5. Write the report
cat > "$report" <<REPORT
# C2PA validation report

File: \`$signed\`
Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)

## Tools

| Tool | Version |
|------|---------|
| c2patool | $c2patool_version |
| provenance | $provenance_version |

## Results

### c2patool (off-the-shelf)

\`\`\`
$(cat "$out_dir/c2patool-detailed.json")
\`\`\`

### provenance verify (custom reader)

\`\`\`
$(cat "$out_dir/verify.json")
\`\`\`

### Custom assertion (provenance inspect)

\`\`\`json
$(cat "$out_dir/assertion.json")
\`\`\`

## Findings

<!-- Fill in after reviewing the output above -->

| Validator | Result | Notes |
|-----------|--------|-------|
| c2patool | | |
| provenance verify | | |
| verify.contentauthenticity.org | | Tested manually: navigate to the site, upload the signed PNG |

## Notes

- \`signingCredential.untrusted\` is expected: the pipeline signs with
  the c2pa-rs SDK sample certificates, not a real trust chain.
- If c2patool reports \`validation_state: Valid\`, the C2PA manifest is
  well-formed and the claim signature checks out. The custom assertion
  \`edu.utdt.td8.zkloc\` is accepted but not rendered by third-party
  verifiers.
- If any rejection occurs, record the reason plainly — a custom
  extension being refused by current tooling is a finding, not a failure.
REPORT

echo
echo "Report written to $report"
echo "Next: upload $signed to https://verify.contentauthenticity.org and record the result in $report"
