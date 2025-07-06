#!/bin/bash

# Find the target (first argument that doesn't start with -)
TARGET=""
ARGS=""
FOUND_TARGET=false

for arg in "$@"; do
    if [[ "$FOUND_TARGET" == "false" && "$arg" != -* ]]; then
        TARGET="$arg"
        FOUND_TARGET=true
    elif [[ "$FOUND_TARGET" == "true" ]]; then
        ARGS="$ARGS $arg"
    fi
done

# Call the real make with ARGS environment variable
if [[ -n "$TARGET" ]]; then
    ARGS="$ARGS" make "$TARGET"
else
    make "$@"
fi 