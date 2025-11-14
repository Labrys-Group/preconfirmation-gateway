#!/bin/bash
# Docker Build Comparison Script
# Compares build times between old and new Dockerfile

set -e

echo "==================================="
echo "Docker Build Performance Comparison"
echo "==================================="
echo ""

# Ensure BuildKit is enabled
export DOCKER_BUILDKIT=1

BIN_NAME=${1:-gateway}
echo "Building binary: $BIN_NAME"
echo ""

# Clean up old builds
echo "Cleaning up old builds..."
docker builder prune -f --filter until=1h > /dev/null 2>&1 || true
echo ""

# Test old Dockerfile (if it exists)
if [ -f "Dockerfile.backup" ] || [ -f "Dockerfile.old" ]; then
    OLD_DOCKERFILE=$([ -f "Dockerfile.backup" ] && echo "Dockerfile.backup" || echo "Dockerfile.old")
    echo "==================================="
    echo "Testing OLD Dockerfile: $OLD_DOCKERFILE"
    echo "==================================="

    START=$(date +%s)
    docker build \
        -f "$OLD_DOCKERFILE" \
        --build-arg BIN_NAME="$BIN_NAME" \
        -t preconfirmation-gateway/${BIN_NAME}:old \
        . || echo "Old build failed"
    END=$(date +%s)
    OLD_TIME=$((END - START))

    echo ""
    echo "Old Dockerfile build time: ${OLD_TIME} seconds"
    echo ""
else
    echo "No old Dockerfile found (Dockerfile.backup or Dockerfile.old)"
    echo "Skipping old build comparison"
    echo ""
    OLD_TIME=0
fi

# Test optimized Dockerfile
echo "==================================="
echo "Testing OPTIMIZED Dockerfile"
echo "==================================="

OPTIMIZED_DOCKERFILE="Dockerfile.optimized"
if [ ! -f "$OPTIMIZED_DOCKERFILE" ]; then
    # Maybe it was already moved to Dockerfile
    if grep -q "mount=type=cache" Dockerfile 2>/dev/null; then
        OPTIMIZED_DOCKERFILE="Dockerfile"
        echo "Using main Dockerfile (appears to be optimized)"
    else
        echo "ERROR: Optimized Dockerfile not found!"
        exit 1
    fi
fi

START=$(date +%s)
docker build \
    -f "$OPTIMIZED_DOCKERFILE" \
    --build-arg BIN_NAME="$BIN_NAME" \
    -t preconfirmation-gateway/${BIN_NAME}:new \
    .
END=$(date +%s)
NEW_TIME=$((END - START))

echo ""
echo "Optimized Dockerfile build time: ${NEW_TIME} seconds"
echo ""

# Show comparison
if [ "$OLD_TIME" -gt 0 ]; then
    echo "==================================="
    echo "COMPARISON RESULTS"
    echo "==================================="
    echo "Old build:       ${OLD_TIME}s"
    echo "Optimized build: ${NEW_TIME}s"
    echo ""

    IMPROVEMENT=$((OLD_TIME - NEW_TIME))
    if [ "$OLD_TIME" -gt 0 ]; then
        PERCENT=$((IMPROVEMENT * 100 / OLD_TIME))
        echo "Time saved:      ${IMPROVEMENT}s (${PERCENT}% faster)"
    fi
    echo ""
fi

# Test incremental build (code change)
echo "==================================="
echo "Testing INCREMENTAL build (simulated code change)"
echo "==================================="
echo ""

# Touch a source file to simulate a change
touch bin/src/main.rs 2>/dev/null || touch crates/gateway/src/lib.rs 2>/dev/null || true

START=$(date +%s)
docker build \
    -f "$OPTIMIZED_DOCKERFILE" \
    --build-arg BIN_NAME="$BIN_NAME" \
    -t preconfirmation-gateway/${BIN_NAME}:incremental \
    .
END=$(date +%s)
INCREMENTAL_TIME=$((END - START))

echo ""
echo "Incremental build time: ${INCREMENTAL_TIME}s"
echo ""

echo "==================================="
echo "SUMMARY"
echo "==================================="
if [ "$OLD_TIME" -gt 0 ]; then
    echo "Old Dockerfile (clean):      ${OLD_TIME}s"
fi
echo "Optimized Dockerfile (clean): ${NEW_TIME}s"
echo "Optimized Dockerfile (incr.): ${INCREMENTAL_TIME}s"
echo ""

if [ "$OLD_TIME" -gt 0 ] && [ "$OLD_TIME" -gt "$NEW_TIME" ]; then
    echo "✅ Optimization successful!"
    PERCENT=$((( OLD_TIME - NEW_TIME ) * 100 / OLD_TIME))
    echo "   Clean builds are ${PERCENT}% faster"
elif [ "$OLD_TIME" -gt 0 ]; then
    echo "⚠️  No improvement detected"
    echo "   Make sure BuildKit is enabled: export DOCKER_BUILDKIT=1"
else
    echo "✅ Optimized Dockerfile builds successfully"
fi

echo ""
echo "💡 Tip: Run this script again to see the benefits of caching!"
echo "    The second run should be much faster due to BuildKit cache."
