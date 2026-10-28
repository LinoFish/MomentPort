/// 旧版 Swift 转换引擎（v1）。v1.2 起转换事实来源是 Rust（rust/，经 RustBackend.swift 调用），
/// 本文件不再被界面调用，仅保留以免破坏 Xcode 工程引用；不要在这里改转换逻辑。
import Foundation

/// Apple MakerNote（iPhone 连拍在文件里的分组标识）.
///
/// 背景（已核实的部分，请勿夸大）：
/// - ExifTool 的 Apple MakerNote 表把 0x000b 定义为 BurstUUID，
///   注释是 "unique ID for all images in a burst"，即同一组连拍共享同一个值。
/// - 0x0015 是 ImageUniqueID，每张照片各不相同。
/// - 0x0001 是 MakerNoteVersion。
/// - 苹果没有公开文档说“只要写了 BurstUUID，相册就一定成组”；
///   相册里的 burstIdentifier / representsBurst 是图库在导入时赋值的，
///   PhotoKit 也没有公开的 setter。本程序写的是文件层唯一有据可查的标识，
///   成组与否必须在导入后用 PhotoKit 回查验证（见 PhotoImporter）。
///
/// 格式（与 ExifTool / exifread 解析的 Apple MakerNote 一致）：
/// - 本身就是一个迷你 TIFF IFD，没有 TIFF 文件头：
///   UInt16 条目数 + 每条 12 字节 + UInt32 下一 IFD（0）。
/// - 字节序与外层 TIFF 相同；值超过 4 字节时，条目里写的是
///   “相对 MakerNote 起始位置的偏移”，不是相对 TIFF 文件头的偏移。
/// - 字符串是 ASCII，count 含结尾的 NUL；为对齐，字符串之间可多补一个 NUL，
///   但 count 不变，解析时只读 count 个字节。
enum AppleMakerNote {
    static let burstUUIDTag: UInt16 = 0x000b
    static let imageUniqueIDTag: UInt16 = 0x0015
    static let versionTag: UInt16 = 0x0001
    /// 合成的 MakerNote 版本号。取 15 是因为它对应 macOS 12 / iOS 15 时代
    /// 常见的 iPhone MakerNote 版本，不是声称这张照片就是某台 iPhone 拍的。
    /// Make / Model 一律保留荣耀原值，不改成 Apple（见 README）。
    static let syntheticVersion: Int32 = 15

    /// 为一组连拍生成一个 BurstUUID（大写、带连字符的标准 UUID 形式，
    /// 与苹果 PhotoIdentifier 的写法一致）。同一组所有帧必须用同一个值。
    static func makeBurstUUID() -> String {
        UUID().uuidString
    }

    /// 为单帧生成一个 ImageUniqueID，每帧各不相同。
    static func makeImageUniqueID() -> String {
        UUID().uuidString
    }

    /// 构造 Apple MakerNote 数据块。
    static func build(burstUUID: String, imageUniqueID: String, byteOrder: TIFFByteOrder) -> Data {
        let burstBytes = Array(burstUUID.utf8) + [0]
        let uniqueBytes = Array(imageUniqueID.utf8) + [0]
        // IFD: 2（条目数）+ 3*12（条目）+ 4（下一 IFD）= 42 字节，字符串从 42 开始。
        let headerSize = 2 + 3 * 12 + 4
        let burstOffset = headerSize
        // 字符串按偶数对齐：奇数长度时多补一个 NUL，解析不受影响。
        let burstPadded = burstBytes.count % 2 == 0 ? burstBytes.count : burstBytes.count + 1
        let uniqueOffset = burstOffset + burstPadded

        var out = Data()
        func append16(_ v: UInt16) { out.append(contentsOf: byteOrder.bytes16(v)) }
        func append32(_ v: UInt32) { out.append(contentsOf: byteOrder.bytes32(v)) }
        func appendEntry(tag: UInt16, type: UInt16, count: UInt32, valueOrOffset: UInt32) {
            append16(tag); append16(type); append32(count); append32(valueOrOffset)
        }

        append16(3)
        // 0x0001 LONG, count 1，值直接写进 4 字节字段。
        appendEntry(tag: versionTag, type: 4, count: 1,
                    valueOrOffset: UInt32(bitPattern: syntheticVersion))
        // 0x000b ASCII，count 含 NUL。
        appendEntry(tag: burstUUIDTag, type: 2, count: UInt32(burstBytes.count),
                    valueOrOffset: UInt32(burstOffset))
        // 0x0015 ASCII，count 含 NUL。
        appendEntry(tag: imageUniqueIDTag, type: 2, count: UInt32(uniqueBytes.count),
                    valueOrOffset: UInt32(uniqueOffset))
        append32(0)

        out.append(contentsOf: burstBytes)
        if burstBytes.count % 2 == 1 { out.append(0) }
        out.append(contentsOf: uniqueBytes)
        if uniqueBytes.count % 2 == 1 { out.append(0) }
        return out
    }

    /// 解析一个 Apple MakerNote 数据块，返回 (burstUUID, imageUniqueID)。
    /// 解析失败返回 (nil, nil)，调用方不得因此丢弃照片，只是不再断言成组。
    static func parse(_ data: Data, byteOrder: TIFFByteOrder) -> (burstUUID: String?, imageUniqueID: String?) {
        guard data.count >= 2 else { return (nil, nil) }
        let count = Int(byteOrder.uint16(data, at: 0))
        guard count > 0, data.count >= 2 + count * 12 + 4 else { return (nil, nil) }
        var burst: String?
        var unique: String?
        for i in 0..<count {
            let off = 2 + i * 12
            guard off + 12 <= data.count else { break }
            let tag = byteOrder.uint16(data, at: off)
            let type = byteOrder.uint16(data, at: off + 2)
            let cnt = Int(byteOrder.uint32(data, at: off + 4))
            let valOff = Int(byteOrder.uint32(data, at: off + 8))
            // 只解析 ASCII 字符串；LONG 等其它类型本程序不依赖。
            guard type == 2, cnt > 0, valOff + cnt <= data.count else { continue }
            let raw = data[valOff..<(valOff + cnt)]
            // 去掉结尾 NUL 再转字符串。
            let trimmed = raw.prefix(while: { $0 != 0 })
            let str = String(bytes: trimmed, encoding: .ascii)
            if tag == burstUUIDTag { burst = str }
            if tag == imageUniqueIDTag { unique = str }
        }
        return (burst, unique)
    }
}
