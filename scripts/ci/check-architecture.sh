#!/usr/bin/env bash
# Architecture checks (ARCH-006): layering, allowed UI imports, unsafe, colours, strings.
# Usage: scripts/ci/check-architecture.sh [--self-test]
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "${1:-}" == "--self-test" ]]; then
    exec python3 "${here}/check_architecture.py" --self-test
fi
python3 "${here}/check_architecture.py" --self-test
exec python3 "${here}/check_architecture.py" "${here}/../.."
