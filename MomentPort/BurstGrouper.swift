import Foundation
import ImageIO

/// 荣耀连拍的分组。
///
/// 荣耀图库里的“连拍组”在导出/另存后就不再是一个文件，
/// 导出来就是一堆单独的 JPEG。因此程序必须从别的信息里把组找回来，
/// 优先级如下（用户在界面里仍可手动合组/拆组——见 ContentView 的说明，
/// 本文件只负责第一次自动分组）：
/// 1. 子文件夹优先：输入文件夹下每个含 ≥2 张照片的子文件夹算一组。
///    这是最可靠的：用户在荣耀图库里每组“另存为”到不同文件夹即可。
/// 2. 平铺文件夹：按拍摄时间排序，相邻两张间隔 ≤ gap 秒归一组，
///    间隔 > gap 秒就切开。荣耀连拍帧间隔通常远小于 1 秒，
///    两次按住快门之间通常不止 2 秒，默认 gap 取 2 秒。
/// 3. 只有 1 张的“组”不算连拍，列为未成组，不写 BurstUUID。
enum BurstGrouper {
    static let imageExtensions: Set<String> = ["jpg", "jpeg"]

    /// 扫描输入文件夹。recursive 为 false 时只读顶层与一层子文件夹，
    /// 避免把整个图库误扫进来。
    static func scan(inputFolder: URL) -> [PhotoFile] {
        var files: [PhotoFile] = []
        let fm = FileManager.default
        guard let enumerator = fm.enumerator(
            at: inputFolder,
            includingPropertiesForKeys: [.isRegularFileKey, .contentModificationDateKey],
            options: [.skipsHiddenFiles]) else { return [] }
        for case let url as URL in enumerator {
            // 只深入一层子文件夹：顶层文件 + 子文件夹里的文件。
            let rel = url.path.replacingOccurrences(of: inputFolder.path + "/", with: "")
            if rel.contains("/") && rel.components(separatedBy: "/").count > 2 { continue }
            guard imageExtensions.contains(url.pathExtension.lowercased()) else { continue }
            guard let info = readPhotoInfo(url: url) else { continue }
            files.append(info)
        }
        return files.sorted {
            if $0.captureTime != $1.captureTime { return $0.captureTime < $1.captureTime }
            return $0.fileName < $1.fileName
        }
    }

    /// 按子文件夹 + 时间切分，返回（连拍组，未成组）。
    static func group(photos: [PhotoFile], inputFolder: URL, gapSeconds: Double) -> ([BurstGroup], [PhotoFile]) {
        // 先按“所在子文件夹”分桶；顶层文件归为 "" 桶。
        var buckets: [String: [PhotoFile]] = [:]
        var bucketOrder: [String] = []
        for p in photos {
            let dir = p.url.deletingLastPathComponent().path
            let key = dir == inputFolder.path ? "" : dir
            if buckets[key] == nil { buckets[key] = []; bucketOrder.append(key) }
            buckets[key]?.append(p)
        }
        var groups: [BurstGroup] = []
        var singles: [PhotoFile] = []
        for key in bucketOrder {
            let bucket = (buckets[key] ?? []).sorted {
                if $0.captureTime != $1.captureTime { return $0.captureTime < $1.captureTime }
                return $0.fileName < $1.fileName
            }
            if key != "" {
                // 子文件夹：整文件夹就是一组（即使只有 1 张也先列出，由调用方过滤）。
                if bucket.count >= 2 {
                    groups.append(BurstGroup(photos: bucket,
                                             sourceDescription: "子文件夹：\(URL(fileURLWithPath: key).lastPathComponent)"))
                } else {
                    singles.append(contentsOf: bucket)
                }
                continue
            }
            // 顶层平铺：按时间间隔切分。
            var current: [PhotoFile] = []
            for p in bucket {
                if let last = current.last,
                   p.captureTime.timeIntervalSince(last.captureTime) > gapSeconds {
                    if current.count >= 2 {
                        groups.append(BurstGroup(photos: current, sourceDescription: "按时间自动分组（间隔 ≤ \(gapSeconds) 秒）"))
                    } else {
                        singles.append(contentsOf: current)
                    }
                    current = []
                }
                current.append(p)
            }
            if current.count >= 2 {
                groups.append(BurstGroup(photos: current, sourceDescription: "按时间自动分组（间隔 ≤ \(gapSeconds) 秒）"))
            } else {
                singles.append(contentsOf: current)
            }
        }
        return (groups, singles)
    }

    // MARK: - 读取拍摄时间与尺寸（只读，不改文件）

    static func readPhotoInfo(url: URL) -> PhotoFile? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
        let exif = props[kCGImagePropertyExifDictionary as String] as? [String: Any] ?? [:]
        let tiff = props[kCGImagePropertyTIFFDictionary as String] as? [String: Any] ?? [:]

        var capture: Date?
        var hasExifDate = false
        if let dateStr = exif[kCGImagePropertyExifDateTimeOriginal as String] as? String,
           let base = parseExifDate(dateStr) {
            var interval = base
            if let sub = exif[kCGImagePropertyExifSubsecTimeOriginal as String] as? String,
               let frac = parseSubSec(sub) {
                interval = base.addingTimeInterval(frac)
            }
            capture = interval
            hasExifDate = true
        } else if let dateStr = tiff[kCGImagePropertyTIFFDateTime as String] as? String,
                  let base = parseExifDate(dateStr) {
            capture = base
            hasExifDate = true
        }
        if capture == nil {
            let attrs = try? FileManager.default.attributesOfItem(atPath: url.path)
            capture = (attrs?[.modificationDate] as? Date) ?? Date()
        }
        var width: Int?
        var height: Int?
        if let w = props[kCGImagePropertyPixelWidth as String] as? Int { width = w }
        if let h = props[kCGImagePropertyPixelHeight as String] as? Int { height = h }
        return PhotoFile(url: url, fileName: url.lastPathComponent,
                         captureTime: capture ?? Date(), hasExifDate: hasExifDate,
                         width: width, height: height)
    }

    static func parseExifDate(_ s: String) -> Date? {
        let f = DateFormatter()
        f.dateFormat = "yyyy:MM:dd HH:mm:ss"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone.current
        return f.date(from: s)
    }

    /// SubSecTimeOriginal 是纯数字字符串，如 "123" 表示 0.123 秒，
    /// "12" 表示 0.12 秒。按位数换算，不假设一定是 3 位毫秒。
    static func parseSubSec(_ s: String) -> Double? {
        let digits = s.filter { $0.isNumber }
        guard !digits.isEmpty, let value = Double(digits) else { return nil }
        return value / pow(10.0, Double(digits.count))
    }
}
