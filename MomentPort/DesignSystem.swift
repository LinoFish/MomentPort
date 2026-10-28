import SwiftUI

/// MomentPort 设计系统（v1.2）。
///
/// 方向（按产品题材定，不套通用模板）：
/// - 这是处理文件的工具，要冷静、可核对，不是营销页。主色沿用
///   MomentPort / MomentStays 已有品牌色：深蓝 #0D1B27、薄荷青 #65DCC5、
///   琥珀 #F3BE72、米白 #F0F6F5。琥珀只做关键帧与提醒，不铺满页面；
///   薄荷青做成功 / 进行中；错误用系统红。
/// - 不用紫色渐变、不用 SaaS 圆角卡片堆、不用表情符号图标（用 SF Symbols）、
///   不用全大写小标签。系统字体即 Dynamic Type，不写死字号。
/// - 母题是连拍“帧条”：每组是一条横向帧带，文件名、时间、组数、帧数、
///   跳过原因、两种验证结果都写在明处。
enum DesignSystem {
    static let spacingSmall: CGFloat = 8
    static let spacingMedium: CGFloat = 16
    static let spacingLarge: CGFloat = 24
    static let cornerRadius: CGFloat = 10
    /// 可点区域最小高度（ui-ux-pro-max 无障碍指南：可交互元素要可辨、可点）。
    static let minTapHeight: CGFloat = 44
}

extension Color {
    static let mpDeepBlue = Color(red: 0x0D / 255, green: 0x1B / 255, blue: 0x27 / 255)
    static let mpMint = Color(red: 0x65 / 255, green: 0xDC / 255, blue: 0xC5 / 255)
    static let mpAmber = Color(red: 0xF3 / 255, green: 0xBE / 255, blue: 0x72 / 255)
    static let mpPaper = Color(red: 0xF0 / 255, green: 0xF6 / 255, blue: 0xF5 / 255)
}

/// 主按钮：深蓝底、白字，禁用时降不透明度（不要让禁用态和正常态长一样）。
struct PrimaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(maxWidth: .infinity, minHeight: DesignSystem.minTapHeight)
            .background(configuration.isPressed ? Color.mpDeepBlue.opacity(0.8) : Color.mpDeepBlue)
            .foregroundColor(.white)
            .cornerRadius(DesignSystem.cornerRadius)
            .opacity(configuration.isPressed ? 0.9 : 1)
    }
}

/// 次按钮：描边样式，同样保证 44pt 可点高度。
struct SecondaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(minHeight: DesignSystem.minTapHeight)
            .padding(.horizontal, DesignSystem.spacingMedium)
            .background(Color.mpPaper)
            .foregroundColor(.mpDeepBlue)
            .cornerRadius(DesignSystem.cornerRadius)
            .overlay(
                RoundedRectangle(cornerRadius: DesignSystem.cornerRadius)
                    .stroke(Color.mpDeepBlue.opacity(0.3), lineWidth: 1)
            )
    }
}

/// 步骤标题。编号是真实流程顺序（1→4），不是装饰性序号。
struct StepHeader: View {
    let step: Int
    let title: String

    var body: some View {
        HStack(spacing: DesignSystem.spacingSmall) {
            Text("\(step)")
                .font(.headline)
                .foregroundColor(.white)
                .frame(width: 24, height: 24)
                .background(Color.mpDeepBlue)
                .clipShape(Circle())
                .accessibilityHidden(true)
            Text(title)
                .font(.headline)
                .accessibilityLabel("第 \(step) 步：\(title)")
        }
    }
}

/// 状态条：错误置顶、成功与提示分开颜色，文字写清原因，不只用颜色传达。
struct StatusBanner: View {
    enum Kind {
        case error, success, info

        var tint: Color {
            switch self {
            case .error: return .red
            case .success: return .mpMint
            case .info: return .mpAmber
            }
        }

        var icon: String {
            switch self {
            case .error: return "exclamationmark.triangle"
            case .success: return "checkmark.circle"
            case .info: return "info.circle"
            }
        }
    }

    let kind: Kind
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: DesignSystem.spacingSmall) {
            Image(systemName: kind.icon)
                .accessibilityHidden(true)
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(DesignSystem.spacingMedium)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(kind.tint.opacity(0.15))
        .overlay(
            RoundedRectangle(cornerRadius: DesignSystem.cornerRadius)
                .stroke(kind.tint.opacity(0.5), lineWidth: 1)
        )
        .accessibilityLabel(text)
    }
}

/// 一帧：文件名 + 时间。关键帧用琥珀描边 + “关键帧”文字，不只靠颜色区分。
struct FrameCell: View {
    let photo: PhotoFile
    let index: Int
    let isKeyFrame: Bool
    let onSelect: () -> Void

    var body: some View {
        Button(action: onSelect) {
            VStack(alignment: .leading, spacing: 4) {
                Image(systemName: "photo")
                    .font(.title3)
                    .accessibilityHidden(true)
                Text(photo.fileName)
                    .font(.caption)
                    .lineLimit(1)
                Text(photo.displayTime)
                    .font(.caption2)
                    .foregroundColor(.secondary)
                if isKeyFrame {
                    Text("关键帧")
                        .font(.caption2)
                        .bold()
                        .foregroundColor(.mpDeepBlue)
                }
            }
            .padding(DesignSystem.spacingSmall)
            .frame(width: 140, alignment: .leading)
            .background(isKeyFrame ? Color.mpAmber.opacity(0.2) : Color.mpPaper)
            .overlay(
                RoundedRectangle(cornerRadius: DesignSystem.cornerRadius)
                    .stroke(isKeyFrame ? Color.mpAmber : Color.secondary.opacity(0.3),
                            lineWidth: isKeyFrame ? 3 : 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("第 \(index + 1) 帧 \(photo.fileName)，\(isKeyFrame ? "关键帧" : "非关键帧")，点按设为关键帧")
    }
}
