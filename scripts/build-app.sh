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
RUST_BIN="$ROOT/rust/target/release/momentport-cli"

command -v cargo >/dev/null 2>&1 || { echo "错误：找不到 cargo，请先装 Rust 工具链。" >&2; exit 1; }
command -v swiftc >/dev/null 2>&1 || { echo "错误：找不到 swiftc，本脚本只在 macOS（含命令行工具）上能跑。" >&2; exit 1; }

echo "==> 1/3 Rust 后端：cargo build --release"
cargo build --release --manifest-path "$ROOT/rust/Cargo.toml"
test -x "$RUST_BIN" || { echo "错误：Rust 二进制未生成：$RUST_BIN" >&2; exit 1; }

echo "==> 2/3 组装 .app bundle：$APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$RUST_BIN" "$APP/Contents/Resources/momentport-cli"
chmod +x "$APP/Contents/Resources/momentport-cli"
# Info.plist 里是 Xcode 变量（$(EXECUTABLE_NAME) 等），swiftc 不会替你展开，
# 直接拷过去系统就找不到可执行文件了，所以这里先替换成真实值再拷。
sed -e 's/\$(EXECUTABLE_NAME)/MomentPort/g' \
    -e 's/\$(PRODUCT_BUNDLE_IDENTIFIER)/com.linofish.MomentPort/g' \
    -e 's/\$(PRODUCT_NAME)/MomentPort/g' \
    -e 's/\$(PRODUCT_BUNDLE_PACKAGE_TYPE)/APPL/g' \
    "$ROOT/MomentPort/Info.plist" > "$APP/Contents/Info.plist"
grep -q 'MomentPort' "$APP/Contents/Info.plist" || { echo "错误：Info.plist 变量替换失败。" >&2; exit 1; }

echo "==> 3/3 swiftc 编 SwiftUI 界面"
# 注意：旧 Swift 转换引擎文件也一起编（它们仍被 MomentPortEngine.Result
# 类型引用），但界面不再调用它们的转换逻辑；转换事实来源是 Rust。
# shellcheck disable=SC2086
swiftc \
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
  -o "$APP/Contents/MacOS/MomentPort"
test -x "$APP/Contents/MacOS/MomentPort" || { echo "错误：Swift 编译产物未生成。" >&2; exit 1; }

echo "==> 临时签名（ad-hoc）"
codesign --force --deep --sign - "$APP" || { echo "错误：临时签名失败，未产出可交付的 .app。" >&2; exit 1; }

echo "完成：$APP"
echo "首次打开若被 Gatekeeper 拦下，请在 Finder 里右键点它选“打开”，不要直接双击。"
