import SwiftUI
import AppKit

/// 界面流程（四步，全部在本地完成，不联网、不上传）：
/// 1. 选荣耀连拍文件夹 → 自动分组，可改间隔、改关键帧。
/// 2. 选输出文件夹 → 转换（原图不动，输出是写了 BurstUUID 的副本）。
/// 3. 一键导入相册 → 程序用 PhotoKit 回查，告诉你到底成组没有。
/// 4. 没成组也不瞒你：日志里会写明，并给出相簿兜底做法。
///
/// 关于“手动合组/拆组”：本版不做复杂的拖拽合组编辑器，
/// 因为分组错误最常见的原因是时间间隔阈值不对或子文件夹没分好，
/// 这两件事用“间隔滑杆 + 子文件夹优先”已经能解决九成情况。
/// 如果仍分错：先把照片按组分到不同子文件夹，再重新选输入文件夹即可。
final class ConverterModel: ObservableObject {
    @Published var inputFolder: URL?
    @Published var outputFolder: URL?
    @Published var groups: [BurstGroup] = []
    @Published var singles: [PhotoFile] = []
    @Published var gapSeconds: Double = 2.0
    @Published var log: String = ""
    @Published var isWorking = false
    @Published var results: [MomentPortEngine.Result] = []
    @Published var verifications: [ImportVerification] = []

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

    func rescan() {
        guard let input = inputFolder else { return }
        let photos = BurstGrouper.scan(inputFolder: input)
        let (g, s) = BurstGrouper.group(photos: photos, inputFolder: input, gapSeconds: gapSeconds)
        groups = g
        singles = s
        appendLog("扫描到 \(photos.count) 张照片，分成 \(g.count) 组连拍，\(s.count) 张未成组。")
        if !s.isEmpty {
            appendLog("未成组（只有 1 张，或与前后间隔超过 \(gapSeconds) 秒）不会写 BurstUUID，转换时会跳过。")
        }
    }

    func convert() {
        guard let out = outputFolder else {
            appendLog("请先选输出文件夹。")
            return
        }
        guard !groups.isEmpty else {
            appendLog("没有可转换的连拍组。")
            return
        }
        isWorking = true
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let res = try MomentPortEngine.convert(groups: self.groups, outputFolder: out) { msg in
                    DispatchQueue.main.async { self.appendLog(msg) }
                }
                DispatchQueue.main.async {
                    self.results = res
                    self.isWorking = false
                    self.appendLog("转换完成：\(res.count) 组。输出在 \(out.path)，每组一个 Burst_xx 文件夹，内含 BurstManifest.json。")
                    self.appendLog("下一步：点“导入到相册并验证”，不要只把文件拖进相册——拖进去无法回查，成没成都不知道。")
                }
            } catch {
                DispatchQueue.main.async {
                    self.isWorking = false
                    self.appendLog("转换失败：\(error.localizedDescription)")
                }
            }
        }
    }

    func importAll() {
        guard !results.isEmpty else {
            appendLog("请先转换，再导入。")
            return
        }
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

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("MomentPort")
                .font(.title2).bold()
            Text("先从荣耀连拍开始：把荣耀手机导出的连拍照片写成苹果相册能识别的连拍格式。原图不动，输出是副本；是否成组，导入后程序会回查并如实告诉你。苹果没有公开连拍文件格式，本程序不承诺 100% 成组，详见 README。")
                .font(.callout)
                .foregroundColor(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            HStack {
                Button("1. 选择荣耀照片文件夹") {
                    model.pickFolder(title: "选择含荣耀连拍照片的文件夹") { url in
                        if let url = url {
                            model.inputFolder = url
                            model.rescan()
                        }
                    }
                }
                Text(model.inputFolder?.path ?? "未选择")
                    .font(.callout).lineLimit(1).truncationMode(.middle)
                Spacer()
                Text("间隔 ≤")
                Stepper(value: $model.gapSeconds, in: 0.5...10, step: 0.5) {
                    Text("\(model.gapSeconds, specifier: "%.1f") 秒")
                }
                .onChange(of: model.gapSeconds) { _ in model.rescan() }
            }

            if !model.groups.isEmpty || !model.singles.isEmpty {
                GroupBox("连拍组（\(model.groups.count) 组）") {
                    List {
                        ForEach($model.groups) { $group in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text("\(group.photos.count) 帧 · \(group.sourceDescription)")
                                        .font(.headline)
                                    Spacer()
                                    Text("关键帧：")
                                    Picker("", selection: $group.keyIndex) {
                                        ForEach(0..<group.photos.count, id: \.self) { i in
                                            Text("第 \(i + 1) 帧").tag(i)
                                        }
                                    }
                                    .frame(width: 130)
                                }
                                Text(group.timeRange).font(.caption).foregroundColor(.secondary)
                                Text(group.photos.map { $0.fileName }.joined(separator: " · "))
                                    .font(.caption).lineLimit(2)
                                Text("BurstUUID：\(group.burstUUID)")
                                    .font(.caption).foregroundColor(.secondary).lineLimit(1)
                            }
                            .padding(.vertical, 2)
                        }
                    }
                    .frame(height: 220)
                }
                if !model.singles.isEmpty {
                    Text("未成组 \(model.singles.count) 张（不转换）：\(model.singles.map { $0.fileName }.joined(separator: "、"))")
                        .font(.caption).foregroundColor(.secondary)
                }
            }

            HStack {
                Button("2. 选择输出文件夹") {
                    model.pickFolder(title: "选择输出文件夹（转换结果放这里）") { url in
                        model.outputFolder = url
                    }
                }
                Text(model.outputFolder?.path ?? "未选择")
                    .font(.callout).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("转换") { model.convert() }
                    .disabled(model.isWorking || model.groups.isEmpty || model.outputFolder == nil)
                Button("3. 导入到相册并验证") { model.importAll() }
                    .disabled(model.isWorking || model.results.isEmpty)
            }

            GroupBox("日志（转换与验证结果都在这里，不只看相册）") {
                ScrollView {
                    Text(model.log.isEmpty ? "还没有日志。先选输入文件夹。" : model.log)
                        .font(.system(.callout, design: .monospaced))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                }
                .frame(height: 180)
            }
            if model.isWorking {
                ProgressView().progressViewStyle(.linear)
            }
            Spacer()
        }
        .padding()
    }
}
