import Foundation

/// SwiftUI 界面与 Rust 后端之间的唯一接线层（v1.2）。
///
/// 转换事实来源是 Rust，界面不再自己算分组、改 EXIF。接线方式是进程调用
/// + JSON，不是 FFI：Rust 二进制的输出格式一旦变了，这里会立刻解码失败，
/// 而不是静默产出错误结果。
///
/// 字段对照（Rust JSON 键名 = 下面 Codable 键名，逐一核对过，改 Rust 那边
/// 必须同步改这里）：
/// - 预览 `--list-json`：gap / groups[{sourceDescription, keyIndex,
///   photos[{fileName, dirKey, captureMs, captureISO, hasExifDate,
///   liveVideoFileName}]}] / singles[同 photo] / livePairs[{photoFileName,
///   photoDirKey, videoFileName}] / unprocessed[{fileName, dirKey, reason}]
/// - 转换报告 `MomentPortReport.json`：groups[{burstUUID, folderName,
///   frameCount, keyFrameFileName, sourceDescription}] / singlesCopied /
///   honestyNote（其余字段界面目前不解析，Codable 会忽略未知键）
/// - 每组清单 `BurstManifest.json`：frames[{fileName}] / keyFrameFileName，
///   用于拼出 PhotoImporter 要的 outputFiles（导入顺序仍按关键帧先导，
///   苹果没有承诺导入顺序决定代表图，见 PhotoImporter）。
///
/// 预览里没有 burstUUID：Rust 每次分组都会新生成 UUID，预览那次的 UUID
/// 不是转换那次的，界面绝不在预览里展示一个“将来会用的 UUID”来误导人。
/// 真正的 burstUUID 只在转换报告里读到。
enum RustBackend {
    enum BackendError: Error, LocalizedError {
        case binaryNotFound
        case previewFailed(code: Int32, stderr: String)
        case convertFailed(code: Int32, stderr: String)
        case reportMissing(URL)
        case decodeFailed(String)

        var errorDescription: String? {
            switch self {
            case .binaryNotFound:
                return "找不到 Rust 后端（momentport-cli）。请先用 scripts/build-app.sh 打包，或在界面里选一次 Rust 二进制的路径。"
            case .previewFailed(let code, let stderr):
                return "Rust 预览失败（退出码 \(code)）：\(stderr)"
            case .convertFailed(let code, let stderr):
                return "Rust 转换失败（退出码 \(code)）：\(stderr)"
            case .reportMissing(let url):
                return "转换后没有找到报告文件：\(url.path)"
            case .decodeFailed(let m):
                return "Rust 输出解析失败：\(m)"
            }
        }
    }

    struct PreviewPhoto: Codable {
        var fileName: String
        var dirKey: String
        var captureMs: Int64
        var captureISO: String
        var hasExifDate: Bool
        var liveVideoFileName: String?
    }

    struct PreviewGroup: Codable {
        var sourceDescription: String
        var keyIndex: Int
        var photos: [PreviewPhoto]
    }

    struct PreviewLivePair: Codable {
        var photoFileName: String
        var photoDirKey: String
        var videoFileName: String
    }

    struct PreviewUnprocessed: Codable {
        var fileName: String
        var dirKey: String
        var reason: String
    }

    struct Preview: Codable {
        var gap: Double
        var groups: [PreviewGroup]
        var singles: [PreviewPhoto]
        var livePairs: [PreviewLivePair]
        var unprocessed: [PreviewUnprocessed]
    }

    struct ReportGroup: Codable {
        var burstUUID: String
        var folderName: String
        var frameCount: Int
        var keyFrameFileName: String
        var sourceDescription: String
    }

    struct Report: Codable {
        var groups: [ReportGroup]
        var singlesCopied: Bool
        var honestyNote: String?
    }

    struct BurstManifest: Codable {
        var keyFrameFileName: String
        var frames: [ManifestFrame]

        struct ManifestFrame: Codable {
            var fileName: String
        }
    }

    /// 查找 Rust 二进制：App Bundle Resources → 用户自选路径 → PATH。
    static func findBinary(customPath: URL?) -> URL? {
        if let custom = customPath,
           FileManager.default.isExecutableFile(atPath: custom.path) {
            return custom
        }
        if let bundled = Bundle.main.url(forResource: "momentport-cli", withExtension: nil),
           FileManager.default.isExecutableFile(atPath: bundled.path) {
            return bundled
        }
        if let pathEnv = ProcessInfo.processInfo.environment["PATH"] {
            for dir in pathEnv.split(separator: ":") {
                let candidate = URL(fileURLWithPath: String(dir))
                    .appendingPathComponent("momentport-cli")
                if FileManager.default.isExecutableFile(atPath: candidate.path) {
                    return candidate
                }
            }
        }
        return nil
    }

    private static func run(binary: URL, args: [String]) throws -> (Int32, String, String) {
        let process = Process()
        process.executableURL = binary
        process.arguments = args
        let outPipe = Pipe()
        let errPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe
        try process.run()
        process.waitUntilExit()
        let out = String(data: outPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let err = String(data: errPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        return (process.terminationStatus, out, err)
    }

    private static func stderrSnippet(_ stderr: String) -> String {
        let trimmed = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "（无错误输出）" }
        return String(trimmed.prefix(500))
    }

    /// 预览：只列分组，不写文件。keyIndexes 按组顺序传入，缺省组用 0。
    static func preview(input: URL, gap: Double, keyIndexes: [Int]?,
                        customBinary: URL?) throws -> Preview {
        guard let binary = findBinary(customPath: customBinary) else {
            throw BackendError.binaryNotFound
        }
        var args = [input.path, "--list-json", "--gap", String(gap)]
        if let keys = keyIndexes, !keys.isEmpty {
            args += ["--key-indexes", keys.map(String.init).joined(separator: ",")]
        }
        let (code, out, err) = try run(binary: binary, args: args)
        guard code == 0 else {
            throw BackendError.previewFailed(code: code, stderr: stderrSnippet(err))
        }
        guard let data = out.data(using: .utf8) else {
            throw BackendError.decodeFailed("预览输出不是 UTF-8")
        }
        do {
            return try JSONDecoder().decode(Preview.self, from: data)
        } catch {
            throw BackendError.decodeFailed(error.localizedDescription)
        }
    }

    /// 转换：跑 Rust CLI，读回 MomentPortReport.json + 各组 BurstManifest.json，
    /// 拼成 PhotoImporter 能直接用的 MomentPortEngine.Result（该类型只是
    /// 导入时的文件清单容器，转换逻辑本身不在 Swift 里）。
    static func convert(input: URL, output: URL, gap: Double, keyIndexes: [Int],
                         copySingles: Bool, customBinary: URL?) throws -> (results: [MomentPortEngine.Result], report: Report) {
        guard let binary = findBinary(customPath: customBinary) else {
            throw BackendError.binaryNotFound
        }
        var args = [input.path, output.path, "--gap", String(gap),
                    "--key-indexes", keyIndexes.map(String.init).joined(separator: ",")]
        if copySingles { args.append("--copy-singles") }
        let (code, _, err) = try run(binary: binary, args: args)
        guard code == 0 else {
            throw BackendError.convertFailed(code: code, stderr: stderrSnippet(err))
        }
        let reportURL = output.appendingPathComponent("MomentPortReport.json")
        guard let reportData = try? Data(contentsOf: reportURL) else {
            throw BackendError.reportMissing(reportURL)
        }
        let report: Report
        do {
            report = try JSONDecoder().decode(Report.self, from: reportData)
        } catch {
            throw BackendError.decodeFailed("MomentPortReport.json：\(error.localizedDescription)")
        }
        var results: [MomentPortEngine.Result] = []
        for (index, group) in report.groups.enumerated() {
            let folder = output.appendingPathComponent(group.folderName)
            let manifestURL = folder.appendingPathComponent("BurstManifest.json")
            var outputFiles: [URL] = []
            var keyOutput: URL?
            if let manifestData = try? Data(contentsOf: manifestURL),
               let manifest = try? JSONDecoder().decode(BurstManifest.self, from: manifestData) {
                outputFiles = manifest.frames.map { folder.appendingPathComponent($0.fileName) }
                if !manifest.keyFrameFileName.isEmpty {
                    keyOutput = folder.appendingPathComponent(manifest.keyFrameFileName)
                }
            }
            results.append(MomentPortEngine.Result(
                groupIndex: index + 1,
                burstUUID: group.burstUUID,
                outputFolder: folder,
                outputFiles: outputFiles,
                keyOutputFile: keyOutput,
                manifestURL: manifestURL,
                skipped: []
            ))
        }
        return (results, report)
    }

    /// 预览 JSON → 界面用的 BurstGroup / PhotoFile（只做映射，不重新分组）。
    static func toGroups(_ preview: Preview, inputFolder: URL) -> ([BurstGroup], [PhotoFile]) {
        func photo(_ p: PreviewPhoto) -> PhotoFile {
            let url = p.dirKey.isEmpty
                ? inputFolder.appendingPathComponent(p.fileName)
                : inputFolder.appendingPathComponent(p.dirKey).appendingPathComponent(p.fileName)
            return PhotoFile(
                url: url,
                fileName: p.fileName,
                captureTime: Date(timeIntervalSince1970: Double(p.captureMs) / 1000),
                hasExifDate: p.hasExifDate,
                width: nil,
                height: nil
            )
        }
        let groups = preview.groups.map { g in
            BurstGroup(
                photos: g.photos.map(photo),
                keyIndex: g.keyIndex,
                burstUUID: "",
                sourceDescription: g.sourceDescription
            )
        }
        return (groups, preview.singles.map(photo))
    }
}
