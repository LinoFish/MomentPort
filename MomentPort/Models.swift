import Foundation

/// 一张待处理的照片。captureTime 由 Rust 读取，用于展示及未识别连拍的时间分组，
/// 取自 EXIF DateTimeOriginal + SubSecTimeOriginal，缺失时用文件修改时间。
struct PhotoFile: Identifiable, Hashable {
    let id = UUID()
    let url: URL
    let fileName: String
    let captureTime: Date
    let hasExifDate: Bool
    let width: Int?
    let height: Int?

    var displayTime: String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss.SSS"
        f.locale = Locale(identifier: "en_US_POSIX")
        return f.string(from: captureTime)
    }
}

/// 一组连拍。keyIndex 默认由 Rust 选荣耀封面/首帧，用户可在界面里覆盖。
/// 程序不靠 keyIndex 改时间，只在导入时先导关键帧（见 PhotoImporter 的说明），
/// 并把 keyIndex 写进 manifest，供用户在相册里“选出最佳”时对照。
struct BurstGroup: Identifiable {
    let id = UUID()
    var photos: [PhotoFile]
    var keyIndex: Int = 0
    var burstUUID: String = UUID().uuidString
    var sourceDescription: String   // 例如“子文件夹 BurstA”或“按时间自动分组”

    var keyPhoto: PhotoFile? {
        guard !photos.isEmpty else { return nil }
        return photos[min(max(keyIndex, 0), photos.count - 1)]
    }
    var timeRange: String {
        guard let first = photos.first, let last = photos.last else { return "" }
        if photos.count == 1 { return first.displayTime }
        return "\(first.displayTime) ～ \(last.displayTime)"
    }
}

/// 转换后每组的清单文件，放在输出子文件夹里，便于复查与重新导入。
struct BurstManifest: Codable {
    var burstUUID: String
    var frameCount: Int
    var keyFrameFileName: String
    var keyFrameIndex: Int
    var sourceDescription: String
    var frames: [FrameInfo]

    struct FrameInfo: Codable {
        var fileName: String
        var sourceFileName: String
        var captureTimeISO: String
        var imageUniqueID: String
        var isKeyFrame: Bool
    }
}

/// 导入验证结果：只报告 PhotoKit 回查到的事实，不推测。
struct ImportVerification: Identifiable {
    let id = UUID()
    var burstUUID: String
    var frameCount: Int
    var importedCount: Int
    var burstIdentifiers: [String]
    var representsBurst: Bool
    var recognizedAsBurst: Bool
    var message: String
}

/// All imported assets must participate; a representative alone is insufficient.
struct BurstVerificationFacts {
    let expectedCount: Int
    let identifiers: [String?]
    let representativeCount: Int

    var uniqueIdentifiers: [String] {
        Array(Set(identifiers.compactMap { id in
            guard let id = id, !id.isEmpty else { return nil }
            return id
        })).sorted()
    }
    var allFramesGrouped: Bool {
        expectedCount > 1 && identifiers.count == expectedCount
            && identifiers.allSatisfy { !($0 ?? "").isEmpty }
            && uniqueIdentifiers.count == 1
    }
    var recognized: Bool { allFramesGrouped && representativeCount == 1 }
}
