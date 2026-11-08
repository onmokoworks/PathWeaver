#!/usr/bin/env bash
# Build actual Tweeq components into the HTML embedded by the native app.
set -euo pipefail
cd "$(dirname "$0")/../tweeq-ui"
if ! command -v npm >/dev/null 2>&1; then
    echo 'Node.js and npm are required to build the Tweeq UI.' >&2
    exit 1
fi
if [[ ! -d node_modules ]] || [[ package-lock.json -nt node_modules/.package-lock.json ]]; then
    npm ci --no-audit --no-fund
fi
npm run build
