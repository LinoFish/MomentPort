#!/bin/sh
# MomentPort v1.2 打包脚本：不用 Xcode IDE。
#
# 做三件事，任何一步失败都直接退出，不产出“看起来成功”的半成品：
# 1. cargo build --release，得到 Rust 后端 momentport-cli；
# 2. 把 Rust 二进制拷进 MomentPort.app/Contents/Resources/（界面按
#    Bundle Resources → 自选路径 → PATH 的顺序找它，见 RustBackend.swift）；
# 3. 用 swiftc 编 SwiftUI 文件成 .app bundle、拷 Info.plist、做临时签名。
#
# 只在 macOS 上能跑（需要 swiftc + macOS SDK）；Linux 上跑到 swiftc 那步
# 必然失败，这是预期，不是脚本 bug。用法：
#   sh scripts/build-app.sh [输出 .app 路径，默认 ./MomentPort.app]
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/MomentPort.app}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
RUST_BIN="$CARGO_TARGET_DIR/release/momentport-cli"
# 不依赖用户 Library 下的默认缓存（受限构建环境中可能不可写）。
export SWIFT_MODULE_CACHE_PATH="${SWIFT_MODULE_CACHE_PATH:-${TMPDIR:-/tmp}/momentport-swift-module-cache}"
export CLANG_MODULE_CACHE_PATH="${CLANG_MODULE_CACHE_PATH:-$SWIFT_MODULE_CACHE_PATH/clang}"
# 与 Info.plist / Xcode 工程的最低系统版本一致，Rust 和 Swift 均适用。
export MACOSX_DEPLOYMENT_TARGET=12.0

command -v cargo >/dev/null 2>&1 || { echo "错误：找不到 cargo，请先装 Rust 工具链。" >&2; exit 1; }
command -v swiftc >/dev/null 2>&1 || { echo "错误：找不到 swiftc，本脚本只在 macOS（含命令行工具）上能跑。" >&2; exit 1; }
case "$APP" in
  *.app) ;;
  *) echo "错误：输出路径必须以 .app 结尾。" >&2; exit 1 ;;
esac
test ! -L "$APP" || { echo "错误：输出路径不能是符号链接。" >&2; exit 1; }
mkdir -p "$(dirname "$APP")" "$SWIFT_MODULE_CACHE_PATH" "$CLANG_MODULE_CACHE_PATH"
APP="$(cd "$(dirname "$APP")" && pwd)/$(basename "$APP")"
# 在输出目录所在文件系统暂存，成功签名后才替换旧产物。
STAGING="$(mktemp -d "$(dirname "$APP")/.momentport-build.XXXXXX")"
BUNDLE="$STAGING/MomentPort.app"
cleanup() {
  status=$?
  if test -d "$STAGING/previous.app" && test ! -e "$APP"; then
    mv "$STAGING/previous.app" "$APP"
  fi
  rm -rf "$STAGING"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM

echo "==> 1/3 Rust 后端：cargo build --release"
cargo build --release --manifest-path "$ROOT/rust/Cargo.toml"
test -x "$RUST_BIN" || { echo "错误：Rust 二进制未生成：$RUST_BIN" >&2; exit 1; }

echo "==> 2/3 组装 .app bundle：$APP"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$RUST_BIN" "$BUNDLE/Contents/Resources/momentport-cli"
chmod +x "$BUNDLE/Contents/Resources/momentport-cli"
# Info.plist 里是 Xcode 变量（$(EXECUTABLE_NAME) 等），swiftc 不会替你展开，
# 直接拷过去系统就找不到可执行文件了，所以这里先替换成真实值再拷。
sed -e 's/\$(EXECUTABLE_NAME)/MomentPort/g' \
    -e 's/\$(PRODUCT_BUNDLE_IDENTIFIER)/com.linofish.MomentPort/g' \
    -e 's/\$(PRODUCT_NAME)/MomentPort/g' \
    -e 's/\$(PRODUCT_BUNDLE_PACKAGE_TYPE)/APPL/g' \
    "$ROOT/MomentPort/Info.plist" > "$BUNDLE/Contents/Info.plist"
plutil -lint "$BUNDLE/Contents/Info.plist"
if grep -q '\$(' "$BUNDLE/Contents/Info.plist"; then
  echo "错误：Info.plist 仍含未展开的 Xcode 变量。" >&2
  exit 1
fi

echo "==> 3/3 swiftc 编 SwiftUI 界面"
# 注意：旧 Swift 转换引擎文件也一起编（它们仍被 MomentPortEngine.Result
# 类型引用），但界面不再调用它们的转换逻辑；转换事实来源是 Rust。
# shellcheck disable=SC2086
swiftc \
  -swift-version 5 -O \
  -target "$(uname -m)-apple-macosx12.0" \
  -module-cache-path "$SWIFT_MODULE_CACHE_PATH" \
  "$ROOT/MomentPort/App.swift" \
  "$ROOT/MomentPort/ContentView.swift" \
  "$ROOT/MomentPort/DesignSystem.swift" \
  "$ROOT/MomentPort/Models.swift" \
  "$ROOT/MomentPort/RustBackend.swift" \
  "$ROOT/MomentPort/PhotoImporter.swift" \
  "$ROOT/MomentPort/BurstGrouper.swift" \
  "$ROOT/MomentPort/JpegExif.swift" \
  "$ROOT/MomentPort/AppleMakerNote.swift" \
  "$ROOT/MomentPort/MomentPortEngine.swift" \
  -framework SwiftUI -framework AppKit -framework Photos -framework ImageIO \
  -o "$BUNDLE/Contents/MacOS/MomentPort"
test -x "$BUNDLE/Contents/MacOS/MomentPort" || { echo "错误：Swift 编译产物未生成。" >&2; exit 1; }

echo "==> 临时签名（ad-hoc）"
codesign --force --sign - "$BUNDLE/Contents/Resources/momentport-cli"
codesign --force --sign - "$BUNDLE" || { echo "错误：临时签名失败，未产出可交付的 .app。" >&2; exit 1; }
codesign --verify --deep --strict "$BUNDLE"
if test -e "$APP"; then
  test -d "$APP" || { echo "错误：输出路径已存在且不是目录。" >&2; exit 1; }
  mv "$APP" "$STAGING/previous.app"
fi
mv "$BUNDLE" "$APP"

echo "完成：$APP"
echo "首次打开若被 Gatekeeper 拦下，请在 Finder 里右键点它选“打开”，不要直接双击。"
