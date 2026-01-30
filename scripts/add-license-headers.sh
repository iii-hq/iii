#!/bin/bash
set -e

HEADER="// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.
"

DIRS=("src" "function-macros/src")
UPDATED=0

for dir in "${DIRS[@]}"; do
  if [ -d "$dir" ]; then
    while IFS= read -r -d '' file; do
      FIRST_LINE=$(head -n 1 "$file")
      if [[ "$FIRST_LINE" != *"Copyright Motia LLC"* ]]; then
        echo "Adding header to: $file"
        CONTENT=$(cat "$file")
        echo -e "${HEADER}\n${CONTENT}" > "$file"
        ((UPDATED++))
      fi
    done < <(find "$dir" -name "*.rs" -type f -print0)
  fi
done

if [ $UPDATED -eq 0 ]; then
  echo "All files already have license headers."
else
  echo "Added headers to $UPDATED file(s)."
fi
