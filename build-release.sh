#!/bin/bash
set -e

PROJECT_NAME="gparallel"
VERSION=$(grep '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')

echo "Building $PROJECT_NAME v$VERSION..."

# Create release directory
mkdir -p release

# Build for different targets
TARGETS=(
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
)

for TARGET in "${TARGETS[@]}"; do
    echo "Building for $TARGET..."
    
    # Check if target is installed
    if ! rustup target list | grep -q "$TARGET (installed)"; then
        echo "Installing target $TARGET..."
        rustup target add $TARGET
    fi
    
    # Build
    if cargo build --release --target $TARGET 2>/dev/null; then
        # Copy binary to release directory with target suffix
        cp "target/$TARGET/release/$PROJECT_NAME" "release/${PROJECT_NAME}-${TARGET}"
        echo "✓ Built $TARGET"
    else
        echo "✗ Failed to build $TARGET (may require cross-compilation tools)"
    fi
done

# Create archives
cd release
for file in *; do
    if [[ -f "$file" ]]; then
        tar -czf "${file}.tar.gz" "$file"
        echo "Created ${file}.tar.gz"
    fi
done

echo "Release builds complete! Check the 'release' directory."