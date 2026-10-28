import SwiftUI
import AppKit

/// 界面流程（四步，全部在本地完成，不联网、不上传）：
/// 1. 选输入文件夹 → Rust 后端分组预览，可改间隔、改关键帧。
/// 2. 选输出文件夹 → Rust 后端转换（原图不动，输出是写了 BurstUUID 的副本）。
/// 3. 一键导入相册 → PhotoKit 回查，告诉你到底成组没有。
/// 4. 没成组也不瞒你：验证结果分开写“已导入”和“已识别为连拍”，
///    并给出相簿兜底做法。
///
/// 转换事实来源是 Rust（见 RustBackend.swift），界面不再自己分组、改 EXIF。
/// 旧 Swift 转换引擎文件仍在工程里，但已不再被界面调用（见各文件头注释）。
/// 分组错了怎么办：先把照片按组分到不同子文件夹，或调时间间隔，再重新预览
/// ——这比在界面里拖拽合组更可靠。
final class ConverterModel: ObservableObject {
    @Published var inputFolder: URL?
    @Published var outputFolder: URL?
    @Published var customBinary: URL?
    @Published var groups: [BurstGroup] = []
    @Published var singles: [PhotoFile] = []
    @Published var unprocessed: [RustBackend.PreviewUnprocessed] = []
    @Published var livePairCount = 0
    @Published var gapSeconds: Double = 2.0
    @Published var copySingles = false
    @Published var log: String = ""
    @Published var errorSummary: String?
    @Published var isWorking = false
    @Published var results: [MomentPortEngine.Result] = []
    @Published var report: RustBackend.Report?
    @Published var verifications: [ImportVerification] = []

    var convertDisabledReason: String? {
        if inputFolder == nil { return "先选输入文件夹。" }
        if groups.isEmpty { return "没有可转换的连拍组：先选含连拍的文件夹，或把间隔调大、或按子文件夹分好组。" }
        if outputFolder == nil { return "先选输出文件夹。" }
        return nil
    }

    var importDisabledReason: String? {
        if results.isEmpty { return "请先转换，再导入。" }
        return nil
    }

    func appendLog(_ s: String) {
        log += s + "\n"
    }

    func pickFolder(title: String, completion: @escaping (URL?) -> Void) {
        let panel = NSOpenPanel()
        panel.title = title
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        if panel.runModal() == .OK {
            completion(panel.url)
        } else {
            completion(nil)
        }
    }

    func pickBinary() {
        let panel = NSOpenPanel()
        panel.title = "选择 Rust 后端（momentport-cli）"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK {
            customBinary = panel.url
            rescan()
        }
    }

    /// 预览：跑 Rust --list-json，不写文件。关键帧在界面里改，不触发重预览
    /// （重预览会把每组关键帧重置回 0）；只有输入文件夹或间隔变了才重预览。
    func rescan() {
        guard let input = inputFolder else { return }
        errorSummary = nil
        isWorking = true
        let gap = gapSeconds
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let preview = try RustBackend.preview(
                    input: input, gap: gap, keyIndexes: nil, customBinary: self.customBinary)
                let (groups, singles) = RustBackend.toGroups(preview, inputFolder: input)
                DispatchQueue.main.async {
                    self.groups = groups
                    self.singles = singles
                    self.unprocessed = preview.unprocessed
                    self.livePairCount = preview.livePairs.count
                    self.isWorking = false
                    self.appendLog("Rust 预览：\(groups.count) 组连拍，\(singles.count) 张单张，\(preview.livePairs.count) 对 live 配对，\(preview.unprocessed.count) 项未处理。")
                }
            } catch {
                DispatchQueue.main.async {
                    self.isWorking = false
                    self.errorSummary = error.localizedDescription
                    self.appendLog("预览失败：\(error.localizedDescription)")
                }
            }
        }
    }

    func convert() {
        guard let out = outputFolder, let input = inputFolder else {
            errorSummary = convertDisabledReason
            return
        }
        guard !groups.isEmpty else {
            errorSummary = convertDisabledReason
            return
        }
        errorSummary = nil
        isWorking = true
        let keys = groups.map { $0.keyIndex }
        let gap = gapSeconds
        let copy = copySingles
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let (res, report) = try RustBackend.convert(
                    input: input, output: out, gap: gap, keyIndexes: keys,
                    copySingles: copy, customBinary: self.customBinary)
                DispatchQueue.main.async {
                    self.results = res
                    self.report = report
                    self.isWorking = false
                    self.appendLog("Rust 转换完成：\(res.count) 组。输出在 \(out.path)，每组一个 Burst_xx 文件夹，内含 BurstManifest.json，顶层有 MomentPortReport.json。")
                    self.appendLog("下一步：点“导入到相册并验证”，不要只把文件拖进相册——拖进去无法回查，成没成都不知道。")
                }
            } catch {
                DispatchQueue.main.async {
                    self.isWorking = false
                    self.errorSummary = error.localizedDescription
                    self.appendLog("转换失败：\(error.localizedDescription)")
                }
            }
        }
    }

    func importAll() {
        guard !results.isEmpty else {
            errorSummary = importDisabledReason
            return
        }
        errorSummary = nil
        isWorking = true
        verifications = []
        let queue = results
        func importNext(_ index: Int) {
            guard index < queue.count else {
                DispatchQueue.main.async {
                    self.isWorking = false
                    self.appendLog("全部导入验证完成。请在相册的“媒体类型 > 连拍”里核对。")
                }
                return
            }
            let r = queue[index]
            PhotoImporter.importAndVerify(result: r) { outcome in
                switch outcome {
                case .success(let v):
                    self.verifications.append(v)
                    self.appendLog("Burst_\(String(format: "%02d", r.groupIndex))：\(v.message)")
                case .failure(let e):
                    self.errorSummary = e.localizedDescription
                    self.appendLog("Burst_\(String(format: "%02d", r.groupIndex)) 导入失败：\(e.localizedDescription)")
                }
                importNext(index + 1)
            }
        }
        importNext(0)
    }
}

struct ContentView: View {
    @StateObject private var model = ConverterModel()

    // 每个区域独立推断类型，不增加布局容器，保留原 VStack 的间距。
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: DesignSystem.spacingMedium) {
                introduction
                errorBanner
                inputSection
                groupsPreview
                singlesPreview
                unprocessedPreview
                outputSection
                importSection
                logSection
            }
            .padding(DesignSystem.spacingMedium)
        }
    }

    @ViewBuilder
    private var introduction: some View {
        Text("MomentPort")
            .font(.title2).bold()
            .accessibilityAddTraits(.isHeader)
        Text("把其他设备导出的连拍照片写成苹果相册能识别的连拍格式。原图不动，输出是副本；是否成组，导入后程序会回查并如实告诉你。苹果没有公开连拍文件格式，本程序不承诺 100% 成组，详见 README。")
            .font(.callout)
            .foregroundColor(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder
    private var errorBanner: some View {
        if let error = model.errorSummary {
            StatusBanner(kind: .error, text: error)
        }
    }

    @ViewBuilder
    private var inputSection: some View {
        // 第 1 步：输入与分组预览
        StepHeader(step: 1, title: "选择输入文件夹，分组预览")
        HStack {
            Button("选择输入文件夹") {
                model.pickFolder(title: "选择含连拍照片的文件夹") { url in
                    if let url = url {
                        model.inputFolder = url
                        model.rescan()
                    }
                }
            }
            .buttonStyle(SecondaryButtonStyle())
            .accessibilityLabel("选择输入文件夹")
            Text(model.inputFolder?.path ?? "未选择")
                .font(.callout).lineLimit(1).truncationMode(.middle)
            Spacer()
            Button("自选 Rust 后端") { model.pickBinary() }
                .buttonStyle(SecondaryButtonStyle())
                .accessibilityLabel("自选 Rust 后端二进制路径")
        }
        HStack {
            Text("平铺文件按时间切分，间隔 ≤")
            Stepper(value: $model.gapSeconds, in: 0.5...10, step: 0.5) {
                Text("\(model.gapSeconds, specifier: "%.1f") 秒")
            }
            .accessibilityLabel("时间间隔，当前 \(model.gapSeconds, specifier: "%.1f") 秒")
            .onChange(of: model.gapSeconds) { _ in model.rescan() }
            Spacer()
            if model.livePairCount > 0 {
                Text("live 配对 \(model.livePairCount) 对（视频原样同拷，不写 BurstUUID）")
                    .font(.caption).foregroundColor(.secondary)
            }
        }
    }

    @ViewBuilder
    private var groupsPreview: some View {
        if !model.groups.isEmpty {
            Text("连拍组（\(model.groups.count) 组）")
                .font(.headline)
            ForEach(model.groups.indices, id: \.self) { groupIndex in
                BurstGroupPreview(group: $model.groups[groupIndex], groupIndex: groupIndex)
                Divider()
            }
        }
    }

    @ViewBuilder
    private var singlesPreview: some View {
        if !model.singles.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text("单张（\(model.singles.count) 张，不转换）")
                    .font(.headline)
                Text("单张不是连拍，不写 BurstUUID；勾选“复制单张”时也只是原样复制，不改 EXIF。")
                    .font(.callout).foregroundColor(.secondary)
                Text(model.singles.map { $0.fileName }.joined(separator: "、"))
                    .font(.caption)
            }
        }
    }

    @ViewBuilder
    private var unprocessedPreview: some View {
        if !model.unprocessed.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text("未处理（\(model.unprocessed.count) 项，不静默丢弃）")
                    .font(.headline)
                ForEach(Array(model.unprocessed.enumerated()), id: \.offset) { _, item in
                    Text("\(item.fileName)：\(item.reason)")
                        .font(.caption)
                        .accessibilityLabel("\(item.fileName)，未处理原因：\(item.reason)")
                }
            }
        }
    }

    @ViewBuilder
    private var outputSection: some View {
        // 第 2 步：输出与转换
        StepHeader(step: 2, title: "选择输出文件夹，转换")
        HStack {
            Button("选择输出文件夹") {
                model.pickFolder(title: "选择输出文件夹（转换结果放这里）") { url in
                    model.outputFolder = url
                }
            }
            .buttonStyle(SecondaryButtonStyle())
            Text(model.outputFolder?.path ?? "未选择")
                .font(.callout).lineLimit(1).truncationMode(.middle)
            Spacer()
        }
        Toggle("同时把单张原样复制到 Singles/", isOn: $model.copySingles)
            .accessibilityLabel("同时把单张原样复制到 Singles 文件夹")
        Button("转换") { model.convert() }
            .buttonStyle(PrimaryButtonStyle())
            .disabled(model.isWorking || model.convertDisabledReason != nil)
            .accessibilityLabel("转换")
        if let reason = model.convertDisabledReason {
            Text(reason).font(.callout).foregroundColor(.secondary)
        }
        if let report = model.report {
            StatusBanner(kind: .success, text: "转换完成：\(report.groups.count) 组。文件层 BurstUUID 已写好；这不等于相册已成组，成组与否看第 3 步的回查。")
            ForEach(report.groups, id: \.folderName) { group in
                Text("\(group.folderName)：\(group.frameCount) 帧，关键帧 \(group.keyFrameFileName)")
                    .font(.callout)
            }
        }
    }

    @ViewBuilder
    private var importSection: some View {
        // 第 3 步：导入与验证（“已导入”和“已识别为连拍”分开写）
        StepHeader(step: 3, title: "导入到相册并验证")
        Button("导入到相册并验证") { model.importAll() }
            .buttonStyle(PrimaryButtonStyle())
            .disabled(model.isWorking || model.importDisabledReason != nil)
            .accessibilityLabel("导入到相册并验证")
        Text("此按钮导入系统照片图库。测试其他图库时，请在 Photos 中手动导入输出文件。")
            .font(.callout).foregroundColor(.secondary)
        if let reason = model.importDisabledReason {
            Text(reason).font(.callout).foregroundColor(.secondary)
        }
        if !model.verifications.isEmpty {
            ForEach(model.verifications) { v in
                VStack(alignment: .leading, spacing: 4) {
                    Text("已导入：\(v.importedCount) / \(v.frameCount) 张")
                        .font(.headline)
                    Text("已识别为连拍：\(v.recognizedAsBurst ? "是" : "否")")
                        .font(.headline)
                        .foregroundColor(v.recognizedAsBurst ? .mpDeepBlue : .secondary)
                    Text(v.message)
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Divider()
            }
        }
    }

    @ViewBuilder
    private var logSection: some View {
        // 日志：仍保留完整记录，但错误摘要已在顶部，不只靠翻日志找错。
        StepHeader(step: 4, title: "日志")
        Text("转换与验证的完整记录都在这里；出错时先看顶部红色错误条。")
            .font(.callout).foregroundColor(.secondary)
        ScrollView {
            Text(model.log.isEmpty ? "还没有日志。先选输入文件夹。" : model.log)
                .font(.system(.callout, design: .monospaced))
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
        }
        .frame(height: 160)
        if model.isWorking {
            ProgressView().progressViewStyle(.linear)
        }
    }
}

/// 绑定原模型中的组，帧条与 Picker 继续共享同一个关键帧索引。
private struct BurstGroupPreview: View {
    @Binding var group: BurstGroup
    let groupIndex: Int

    var body: some View {
        VStack(alignment: .leading, spacing: DesignSystem.spacingSmall) {
            HStack {
                Text("\(group.photos.count) 帧 · \(group.sourceDescription)")
                    .font(.headline)
                Spacer()
                Text("关键帧：\(group.keyPhoto?.fileName ?? "")")
                    .font(.callout)
            }
            Text(group.timeRange).font(.caption).foregroundColor(.secondary)
            ScrollView(.horizontal, showsIndicators: true) {
                HStack(spacing: DesignSystem.spacingSmall) {
                    ForEach(Array(group.photos.enumerated()), id: \.element.id) { i, photo in
                        FrameCell(photo: photo, index: i, isKeyFrame: i == group.keyIndex) {
                            group.keyIndex = i
                        }
                    }
                }
            }
            Picker("关键帧", selection: $group.keyIndex) {
                ForEach(0..<group.photos.count, id: \.self) { i in
                    Text("第 \(i + 1) 帧：\(group.photos[i].fileName)").tag(i)
                }
            }
            .accessibilityLabel("第 \(groupIndex + 1) 组关键帧选择")
            .labelsHidden()
        }
    }
}
