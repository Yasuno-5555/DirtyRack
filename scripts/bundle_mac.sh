#!/bin/bash
set -e

# DirtyRack macOS Bundler
APP_NAME="DirtyRack"
BUILD_DIR="target/release"
BUNDLE_DIR="dist/${APP_NAME}.app"
MACOS_DIR="${BUNDLE_DIR}/Contents/MacOS"
RESOURCES_DIR="${BUNDLE_DIR}/Contents/Resources"

echo "⚡ Building DirtyRack App Bundle..."

# Create directory structure
mkdir -p "${MACOS_DIR}"
mkdir -p "${RESOURCES_DIR}/modules"
mkdir -p "${RESOURCES_DIR}/docs"

# Copy binary
cp "${BUILD_DIR}/dirtyrack" "${MACOS_DIR}/"

# Copy default modules
# (In a real scenario, we'd build all modules here)
cp "${BUILD_DIR}/libexample_thirdparty_module.dylib" "${RESOURCES_DIR}/modules/" || true

# Copy documentation
cp docs/*.md "${RESOURCES_DIR}/docs/"

# Create Info.plist
cat > "${BUNDLE_DIR}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>dirtyrack</string>
    <key>CFBundleIdentifier</key>
    <string>com.dirtyrack.app</string>
    <key>CFBundleName</key>
    <string>DirtyRack</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
</dict>
</plist>
EOF

echo "✅ ${APP_NAME}.app created in dist/"
echo "🚀 Run with: open ${BUNDLE_DIR}"
