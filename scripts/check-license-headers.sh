#!/bin/bash
set -e

DIRS=("src" "function-macros/src")
MISSING_FILES=()

for dir in "${DIRS[@]}"; do
  if [ -d "$dir" ]; then
    while IFS= read -r -d '' file; do
      FIRST_LINE=$(head -n 1 "$file")
      if [[ "$FIRST_LINE" != *"Copyright Motia LLC"* ]]; then
        MISSING_FILES+=("$file")
      fi
    done < <(find "$dir" -name "*.rs" -type f -print0)
  fi
done

if [ ${#MISSING_FILES[@]} -gt 0 ]; then
  echo "Files missing license header:"
  printf '  %s\n' "${MISSING_FILES[@]}"
  echo ""
  echo "Run: ./scripts/add-license-headers.sh"
  exit 1
fi

echo "All engine Rust files have license headers."
