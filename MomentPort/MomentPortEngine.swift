import Foundation

/// 转换引擎：只做三件事——复制、写 BurstUUID、写清单。
/// 不重新编码像素，不改 Make / Model，不改拍摄时间与 GPS，
/// 不删除原图（输出到另一个文件夹，原文件夹原封不动）。
enum MomentPortEngine {
    struct Result {
        var groupIndex: Int
        var burstUUID: String
        var outputFolder: URL
        var outputFiles: [URL]
        var keyOutputFile: URL?
        var manifestURL: URL
        var skipped: [(fileName: String, reason: String)]
    }

    static func convert(groups: [BurstGroup], outputFolder: URL,
                        progress: @escaping (String) -> Void) throws -> [Result] {
        let fm = FileManager.default
        try fm.createDirectory(at: outputFolder, withIntermediateDirectories: true)
        var results: [Result] = []
        for (index, group) in groups.enumerated() {
            let folderName = String(format: "Burst_%02d", index + 1)
            let folder = outputFolder.appendingPathComponent(folderName)
            try fm.createDirectory(at: folder, withIntermediateDirectories: true)
            var outputs: [URL] = []
            var keyOutput: URL?
            var skipped: [(String, String)] = []
            var frames: [BurstManifest.FrameInfo] = []
            let iso = ISO8601DateFormatter()
            for (frameIndex, photo) in group.photos.enumerated() {
                let data: Data
                do {
                    data = try Data(contentsOf: photo.url)
                } catch {
                    skipped.append((photo.fileName, "读取失败：\(error.localizedDescription)"))
                    continue
                }
                let imageUniqueID = AppleMakerNote.makeImageUniqueID()
                let converted: Data
                do {
                    converted = try JpegConverter.convert(
                        data: data, burstUUID: group.burstUUID, imageUniqueID: imageUniqueID)
                } catch {
                    skipped.append((photo.fileName, "不是可改写的 JPEG：\(error.localizedDescription)"))
                    continue
                }
                // 校验：刚写完立刻读回，BurstUUID 必须与本组一致，否则不算成功。
                let back = JpegConverter.readBurstInfo(data: converted)
                guard back.burstUUID == group.burstUUID, back.imageUniqueID == imageUniqueID else {
                    skipped.append((photo.fileName, "写完后读回校验失败，已跳过该帧"))
                    continue
                }
                // 文件名保留原名，避免重命名打乱用户的文件管理；
                // 若同组内有重名（不同子文件夹同名），加帧序号后缀。
                var destName = photo.fileName
                if fm.fileExists(atPath: folder.appendingPathComponent(destName).path) {
                    let stem = (photo.fileName as NSString).deletingPathExtension
                    let ext = (photo.fileName as NSString).pathExtension
                    destName = "\(stem)_\(String(format: "%02d", frameIndex + 1)).\(ext)"
                }
                let dest = folder.appendingPathComponent(destName)
                do {
                    try converted.write(to: dest, options: .atomic)
                } catch {
                    skipped.append((photo.fileName, "写入失败：\(error.localizedDescription)"))
                    continue
                }
                outputs.append(dest)
                if frameIndex == group.keyIndex { keyOutput = dest }
                frames.append(BurstManifest.FrameInfo(
                    fileName: destName, sourceFileName: photo.fileName,
                    captureTimeISO: iso.string(from: photo.captureTime),
                    imageUniqueID: imageUniqueID,
                    isKeyFrame: frameIndex == group.keyIndex))
                progress("已转换 \(folderName) 第 \(frameIndex + 1)/\(group.photos.count) 帧：\(destName)")
            }
            let keyName = group.keyPhoto?.fileName ?? ""
            let manifest = BurstManifest(
                burstUUID: group.burstUUID, frameCount: outputs.count,
                keyFrameFileName: keyName, keyFrameIndex: group.keyIndex,
                sourceDescription: group.sourceDescription, frames: frames)
            let manifestURL = folder.appendingPathComponent("BurstManifest.json")
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            if let json = try? encoder.encode(manifest) {
                try? json.write(to: manifestURL, options: .atomic)
            }
            results.append(Result(groupIndex: index + 1, burstUUID: group.burstUUID,
                                  outputFolder: folder, outputFiles: outputs,
                                  keyOutputFile: keyOutput,
                                  manifestURL: manifestURL, skipped: skipped))
        }
        return results
    }
}
