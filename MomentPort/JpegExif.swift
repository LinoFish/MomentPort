import Foundation

// MARK: - TIFF 基础

enum TIFFByteOrder {
    case little, big

    func bytes16(_ v: UInt16) -> [UInt8] {
        switch self {
        case .little: return [UInt8(v & 0xff), UInt8((v >> 8) & 0xff)]
        case .big: return [UInt8((v >> 8) & 0xff), UInt8(v & 0xff)]
        }
    }
    func bytes32(_ v: UInt32) -> [UInt8] {
        switch self {
        case .little:
            return [UInt8(v & 0xff), UInt8((v >> 8) & 0xff),
                    UInt8((v >> 16) & 0xff), UInt8((v >> 24) & 0xff)]
        case .big:
            return [UInt8((v >> 24) & 0xff), UInt8((v >> 16) & 0xff),
                    UInt8((v >> 8) & 0xff), UInt8(v & 0xff)]
        }
    }
    func uint16(_ data: Data, at offset: Int) -> UInt16 {
        guard offset + 2 <= data.count else { return 0 }
        switch self {
        case .little: return UInt16(data[offset]) | (UInt16(data[offset + 1]) << 8)
        case .big: return (UInt16(data[offset]) << 8) | UInt16(data[offset + 1])
        }
    }
    func uint32(_ data: Data, at offset: Int) -> UInt32 {
        guard offset + 4 <= data.count else { return 0 }
        switch self {
        case .little:
            return UInt32(data[offset]) | (UInt32(data[offset + 1]) << 8)
                | (UInt32(data[offset + 2]) << 16) | (UInt32(data[offset + 3]) << 24)
        case .big:
            return (UInt32(data[offset]) << 24) | (UInt32(data[offset + 1]) << 16)
                | (UInt32(data[offset + 2]) << 8) | UInt32(data[offset + 3])
        }
    }
}

/// 一个 TIFF 条目：值以原始字节保存，重建时不再解释其语义，
/// 因此原照片的曝光、GPS、方向等信息可以原样保留。
struct TIFFEntry {
    var tag: UInt16
    var type: UInt16
    var count: UInt32
    var value: Data   // 长度应为 count * typeSize（SHORT/LONG 等按 TIFF 规范）
}

func tiffTypeSize(_ type: UInt16) -> Int {
    switch type {
    case 1, 2, 6, 7: return 1
    case 3, 8: return 2
    case 4, 9, 11: return 4
    case 5, 10, 12: return 8
    default: return 1
    }
}

struct ParsedTIFF {
    var byteOrder: TIFFByteOrder
    var ifd0: [TIFFEntry]
    var exif: [TIFFEntry]?
    var gps: [TIFFEntry]?
    var interop: [TIFFEntry]?
    var ifd1: [TIFFEntry]?
    var thumbnailJPEG: Data?
}

enum JpegExifError: Error, LocalizedError {
    case notJPEG
    case noExifSegment
    case corruptTIFF(String)
    case unsupportedFormat(String)

    var errorDescription: String? {
        switch self {
        case .notJPEG: return "不是 JPEG 文件"
        case .noExifSegment: return "找不到 EXIF 段"
        case .corruptTIFF(let m): return "TIFF 结构异常：\(m)"
        case .unsupportedFormat(let m): return "不支持的格式：\(m)"
        }
    }
}

// MARK: - TIFF 解析

struct TIFFParser {
    static func parse(_ tiff: Data) throws -> ParsedTIFF {
        guard tiff.count >= 8 else { throw JpegExifError.corruptTIFF("TIFF 太短") }
        let order: TIFFByteOrder
        if tiff[0] == 0x49 && tiff[1] == 0x49 {
            order = .little
        } else if tiff[0] == 0x4d && tiff[1] == 0x4d {
            order = .big
        } else {
            throw JpegExifError.corruptTIFF("未知的字节序标记")
        }
        guard order.uint16(tiff, at: 2) == 42 else {
            throw JpegExifError.corruptTIFF("TIFF 魔数不是 42")
        }
        let ifd0Offset = Int(order.uint32(tiff, at: 4))
        let (ifd0, nextOffset) = try parseIFD(tiff, at: ifd0Offset, order: order)

        var exif: [TIFFEntry]?
        var gps: [TIFFEntry]?
        var interop: [TIFFEntry]?
        if let e = ifd0.first(where: { $0.tag == 0x8769 }) {
            let pointer = readUInt32FromValue(e.value, order: order)
            let (entries, _) = try parseIFD(tiff, at: Int(pointer), order: order)
            exif = entries
            if let ip = entries.first(where: { $0.tag == 0xa005 }) {
                let ioff = readUInt32FromValue(ip.value, order: order)
                let (ientries, _) = try parseIFD(tiff, at: Int(ioff), order: order)
                interop = ientries
            }
        }
        if let g = ifd0.first(where: { $0.tag == 0x8825 }) {
            let pointer = readUInt32FromValue(g.value, order: order)
            let (entries, _) = try parseIFD(tiff, at: Int(pointer), order: order)
            gps = entries
        }

        var ifd1: [TIFFEntry]?
        var thumbnail: Data?
        if nextOffset != 0 {
            let (entries, _) = try parseIFD(tiff, at: nextOffset, order: order)
            // 只有“JPEG 缩略图”这种简单 IFD1 才保留；带 StripOffsets 的
            // 复杂缩略图直接丢弃（主图不受影响，相册会重新生成缩略图）。
            let hasStrip = entries.contains(where: { $0.tag == 0x0111 || $0.tag == 0x0117 })
            if !hasStrip,
               let fmt = entries.first(where: { $0.tag == 0x0201 }),
               let len = entries.first(where: { $0.tag == 0x0202 }) {
                let toff = Int(readUInt32FromValue(fmt.value, order: order))
                let tlen = Int(readUInt32FromValue(len.value, order: order))
                if toff + tlen <= tiff.count, tlen > 0 {
                    thumbnail = tiff[toff..<(toff + tlen)]
                    ifd1 = entries.filter { $0.tag != 0x0201 && $0.tag != 0x0202 }
                }
            }
        }
        // IFD0 里去掉子 IFD 指针，重建时再重新计算。
        let cleanIFD0 = ifd0.filter { $0.tag != 0x8769 && $0.tag != 0x8825 }
        let cleanExif = exif?.filter { $0.tag != 0xa005 && $0.tag != 0x927c }
        return ParsedTIFF(byteOrder: order, ifd0: cleanIFD0, exif: cleanExif,
                          gps: gps, interop: interop, ifd1: ifd1, thumbnailJPEG: thumbnail)
    }

    private static func readUInt32FromValue(_ value: Data, order: TIFFByteOrder) -> UInt32 {
        guard value.count >= 4 else { return 0 }
        // value 在解析时已按“值本身”保存：LONG 类型的值就是 4 字节本身，
        // 按 TIFF 字节序解码。
        switch order {
        case .little:
            return UInt32(value[0]) | (UInt32(value[1]) << 8)
                | (UInt32(value[2]) << 16) | (UInt32(value[3]) << 24)
        case .big:
            return (UInt32(value[0]) << 24) | (UInt32(value[1]) << 16)
                | (UInt32(value[2]) << 8) | UInt32(value[3])
        }
    }

    private static func parseIFD(_ tiff: Data, at offset: Int, order: TIFFByteOrder)
        throws -> ([TIFFEntry], Int) {
        guard offset + 2 <= tiff.count else {
            throw JpegExifError.corruptTIFF("IFD 偏移超出范围")
        }
        let count = Int(order.uint16(tiff, at: offset))
        guard offset + 2 + count * 12 + 4 <= tiff.count else {
            throw JpegExifError.corruptTIFF("IFD 条目超出范围")
        }
        var entries: [TIFFEntry] = []
        for i in 0..<count {
            let eoff = offset + 2 + i * 12
            let tag = order.uint16(tiff, at: eoff)
            let type = order.uint16(tiff, at: eoff + 2)
            let cnt = order.uint32(tiff, at: eoff + 4)
            let valueSize = Int(cnt) * tiffTypeSize(type)
            let value: Data
            if valueSize <= 4 {
                // 值直接存在 4 字节字段里，取前 valueSize 字节。
                value = tiff[(eoff + 8)..<(eoff + 8 + valueSize)]
            } else {
                let voff = Int(order.uint32(tiff, at: eoff + 8))
                guard voff + valueSize <= tiff.count else {
                    throw JpegExifError.corruptTIFF("条目 0x\(String(tag, radix: 16)) 的值超出范围")
                }
                value = tiff[voff..<(voff + valueSize)]
            }
            entries.append(TIFFEntry(tag: tag, type: type, count: cnt, value: value))
        }
        let next = Int(order.uint32(tiff, at: offset + 2 + count * 12))
        return (entries, next)
    }
}

// MARK: - TIFF 重建（只改 MakerNote，其余条目原样）

struct TIFFBuilder {
    static func build(parsed: ParsedTIFF, makerNote: Data, pixelWidth: Int?, pixelHeight: Int?) -> Data {
        let order = parsed.byteOrder
        var ifd0 = parsed.ifd0
        var exif = parsed.exif ?? []
        let gps = parsed.gps
        let interop = parsed.interop
        var ifd1 = parsed.ifd1

        // 补上 MakerNote 条目（0x927C，UNDEFINED）。
        exif.append(TIFFEntry(tag: 0x927c, type: 7, count: UInt32(makerNote.count), value: makerNote))
        // 没有 EXIF 时，尽量补拍摄时间与尺寸，避免相册把照片排到错误位置。
        // 有 EXIF 时不碰原有时间，排序以原时间为准。
        if parsed.exif == nil {
            if let w = pixelWidth, let h = pixelHeight {
                exif.append(TIFFEntry(tag: 0xa002, type: 4, count: 1, value: Data(order.bytes32(UInt32(w)))))
                exif.append(TIFFEntry(tag: 0xa003, type: 4, count: 1, value: Data(order.bytes32(UInt32(h)))))
            }
        }
        // 子 IFD 指针条目（值在布局算出后再填，先占位）。
        ifd0.append(TIFFEntry(tag: 0x8769, type: 4, count: 1, value: Data([0, 0, 0, 0])))
        if gps != nil {
            ifd0.append(TIFFEntry(tag: 0x8825, type: 4, count: 1, value: Data([0, 0, 0, 0])))
        }
        if interop != nil {
            exif.append(TIFFEntry(tag: 0xa005, type: 4, count: 1, value: Data([0, 0, 0, 0])))
        }
        if parsed.thumbnailJPEG != nil, ifd1 != nil {
            ifd1?.append(TIFFEntry(tag: 0x0201, type: 4, count: 1, value: Data([0, 0, 0, 0])))
            ifd1?.append(TIFFEntry(tag: 0x0202, type: 4, count: 1, value: Data([0, 0, 0, 0])))
        }
        ifd0.sort { $0.tag < $1.tag }
        exif.sort { $0.tag < $1.tag }
        let gpsSorted = gps?.sorted { $0.tag < $1.tag }
        let interopSorted = interop?.sorted { $0.tag < $1.tag }
        ifd1?.sort { $0.tag < $1.tag }

        func ifdSize(_ n: Int) -> Int { 2 + n * 12 + 4 }
        func valuesSize(_ entries: [TIFFEntry]) -> Int {
            entries.reduce(0) { $0 + ($1.value.count > 4 ? (($1.value.count + 1) / 2 * 2) : 0) }
        }
        // 布局：Header(8) | IFD0 | IFD0值 | Exif | Exif值 | GPS | GPS值 | Interop | Interop值 | IFD1 | IFD1值 | 缩略图
        let ifd0Offset = 8
        let ifd0ValuesOffset = ifd0Offset + ifdSize(ifd0.count)
        let exifOffset = ifd0ValuesOffset + valuesSize(ifd0)
        let exifValuesOffset = exifOffset + ifdSize(exif.count)
        var cursor = exifValuesOffset + valuesSize(exif)
        var gpsOffset = 0, gpsValuesOffset = 0
        if let g = gpsSorted {
            gpsOffset = cursor
            gpsValuesOffset = gpsOffset + ifdSize(g.count)
            cursor = gpsValuesOffset + valuesSize(g)
        }
        var interopOffset = 0, interopValuesOffset = 0
        if let it = interopSorted {
            interopOffset = cursor
            interopValuesOffset = interopOffset + ifdSize(it.count)
            cursor = interopValuesOffset + valuesSize(it)
        }
        var ifd1Offset = 0, ifd1ValuesOffset = 0
        if let f1 = ifd1 {
            ifd1Offset = cursor
            ifd1ValuesOffset = ifd1Offset + ifdSize(f1.count)
            cursor = ifd1ValuesOffset + valuesSize(f1)
        }
        var thumbOffset = 0
        if let thumb = parsed.thumbnailJPEG {
            thumbOffset = cursor
            cursor += (thumb.count + 1) / 2 * 2
        }
        // 把指针条目的值填成算出的偏移。
        func withPointer(_ entries: [TIFFEntry], tag: UInt16, offset: Int) -> [TIFFEntry] {
            entries.map { e in
                if e.tag == tag {
                    return TIFFEntry(tag: e.tag, type: e.type, count: e.count,
                                     value: Data(order.bytes32(UInt32(offset))))
                }
                return e
            }
        }
        ifd0 = withPointer(ifd0, tag: 0x8769, offset: exifOffset)
        if gps != nil { ifd0 = withPointer(ifd0, tag: 0x8825, offset: gpsOffset) }
        exif = withPointer(exif, tag: 0xa005, offset: interopOffset)
        if parsed.thumbnailJPEG != nil, ifd1 != nil {
            ifd1 = withPointer(ifd1!, tag: 0x0201, offset: thumbOffset)
            ifd1 = withPointer(ifd1!, tag: 0x0202, offset: parsed.thumbnailJPEG?.count ?? 0)
        }

        var out = Data()
        func append16(_ v: UInt16) { out.append(contentsOf: order.bytes16(v)) }
        func append32(_ v: UInt32) { out.append(contentsOf: order.bytes32(v)) }
        func writeIFD(_ entries: [TIFFEntry], nextOffset: Int, valueStart: Int) {
            // 调用方保证 out.count 已到达本 IFD 的起始位置；先写条目，再写值区。
            append16(UInt16(entries.count))
            var valueCursor = valueStart
            var pending: [(offset: Int, value: Data)] = []
            for e in entries {
                append16(e.tag); append16(e.type); append32(e.count)
                if e.value.count <= 4 {
                    var field = [UInt8](e.value)
                    while field.count < 4 { field.append(0) }
                    out.append(contentsOf: field)
                } else {
                    append32(UInt32(valueCursor))
                    pending.append((valueCursor, e.value))
                    valueCursor += (e.value.count + 1) / 2 * 2
                }
            }
            append32(UInt32(nextOffset))
            for p in pending {
                // 正常情况下 out.count == p.offset；若有偏差，先补零对齐。
                if out.count < p.offset {
                    out.append(contentsOf: [UInt8](repeating: 0, count: p.offset - out.count))
                }
                out.append(p.value)
                if p.value.count % 2 == 1 { out.append(0) }
            }
        }

        // TIFF 文件头。
        switch order {
        case .little: out.append(contentsOf: [0x49, 0x49])
        case .big: out.append(contentsOf: [0x4d, 0x4d])
        }
        append16(42); append32(UInt32(ifd0Offset))
        writeIFD(ifd0, nextOffset: ifd1Offset, valueStart: ifd0ValuesOffset)
        writeIFD(exif, nextOffset: 0, valueStart: exifValuesOffset)
        if let g = gpsSorted { writeIFD(g, nextOffset: 0, valueStart: gpsValuesOffset) }
        if let it = interopSorted { writeIFD(it, nextOffset: 0, valueStart: interopValuesOffset) }
        if let f1 = ifd1 { writeIFD(f1, nextOffset: 0, valueStart: ifd1ValuesOffset) }
        if let thumb = parsed.thumbnailJPEG {
            if out.count < thumbOffset {
                out.append(contentsOf: [UInt8](repeating: 0, count: thumbOffset - out.count))
            }
            out.append(thumb)
            if thumb.count % 2 == 1 { out.append(0) }
        }
        return out
    }
}

// MARK: - JPEG 段级改写（像素数据不动，只换 EXIF 段）

enum JpegConverter {
    /// 返回改写后的完整 JPEG 数据。像素扫描数据原样复制，不重新编码。
    static func convert(data: Data, burstUUID: String, imageUniqueID: String) throws -> Data {
        guard data.count >= 2, data[0] == 0xff, data[1] == 0xd8 else {
            throw JpegExifError.notJPEG
        }
        var out = Data()
        out.append(contentsOf: [0xff, 0xd8])
        var pos = 2
        var exifReplaced = false
        var pixelWidth: Int?
        var pixelHeight: Int?

        func segmentLength(at p: Int) -> Int {
            guard p + 4 <= data.count else { return 0 }
            return (Int(data[p + 2]) << 8) | Int(data[p + 3])
        }

        while pos + 2 <= data.count {
            guard data[pos] == 0xff else { break }
            let marker = data[pos + 1]
            // 独立标记：SOI / EOI / RSTn / TEM，没有长度字段。
            if marker == 0xd9 {
                out.append(contentsOf: data[pos..<(pos + 2)])
                pos += 2
                break
            }
            if marker == 0x01 || (0xd0...0xd7).contains(marker) {
                out.append(contentsOf: data[pos..<(pos + 2)])
                pos += 2
                continue
            }
            // SOS：从这里起全部是扫描数据，原样复制到文件尾，结束解析。
            if marker == 0xda {
                let len = segmentLength(at: pos)
                guard len >= 2, pos + 2 + len <= data.count else { break }
                out.append(contentsOf: data[pos...])
                pos = data.count
                break
            }
            let len = segmentLength(at: pos)
            guard len >= 2, pos + 2 + len <= data.count else { break }
            let segmentEnd = pos + 2 + len
            let payloadStart = pos + 4
            let payloadEnd = segmentEnd

            // SOF0/1/2：记录尺寸，供无 EXIF 时补 Pixel 尺寸用。
            if marker == 0xc0 || marker == 0xc1 || marker == 0xc2,
               payloadEnd - payloadStart >= 5 {
                pixelHeight = (Int(data[payloadStart + 1]) << 8) | Int(data[payloadStart + 2])
                pixelWidth = (Int(data[payloadStart + 3]) << 8) | Int(data[payloadStart + 4])
            }

            let isExifAPP1 = marker == 0xe1
                && payloadEnd - payloadStart >= 6
                && data[payloadStart] == 0x45 // 'E'
                && data[payloadStart + 1] == 0x78 // 'x'
                && data[payloadStart + 2] == 0x69 // 'i'
                && data[payloadStart + 3] == 0x66 // 'f'
                && data[payloadStart + 4] == 0 && data[payloadStart + 5] == 0

            if isExifAPP1 && !exifReplaced {
                let tiffData = data[(payloadStart + 6)..<payloadEnd]
                let parsed = try TIFFParser.parse(tiffData)
                let makerNote = AppleMakerNote.build(
                    burstUUID: burstUUID, imageUniqueID: imageUniqueID,
                    byteOrder: parsed.byteOrder)
                let newTIFF = TIFFBuilder.build(parsed: parsed, makerNote: makerNote,
                                                pixelWidth: pixelWidth, pixelHeight: pixelHeight)
                var newPayload = Data("Exif".utf8)
                newPayload.append(contentsOf: [0, 0])
                newPayload.append(newTIFF)
                // APP1 段长度 = 2（长度字段本身）+ payload 长度。
                let newLen = newPayload.count + 2
                guard newLen <= 0xffff else {
                    throw JpegExifError.unsupportedFormat("重建后的 EXIF 段超过 64KB")
                }
                out.append(contentsOf: [0xff, 0xe1, UInt8((newLen >> 8) & 0xff), UInt8(newLen & 0xff)])
                out.append(newPayload)
                exifReplaced = true
            } else {
                out.append(contentsOf: data[pos..<segmentEnd])
            }
            pos = segmentEnd
        }

        if !exifReplaced {
            // 原图没有 EXIF：新建一个最小 TIFF，插在 SOI 之后。
            // 注意：此时 pixelWidth/pixelHeight 可能还没读到（SOF 在后面），
            // 因此这里只写 MakerNote 与拍摄时间占位，尺寸由相册从 SOF 读取。
            // 为避免两段式逻辑出错，改为：先完整扫描一遍拿尺寸，再重建。
            // 简单起见，若本轮未遇到 SOF（极少见），仍可正常输出，只是没有尺寸条目。
            let order: TIFFByteOrder = .big // iPhone 常用 big-endian，与新 TIFF 一致
            let empty = ParsedTIFF(byteOrder: order, ifd0: [], exif: nil, gps: nil,
                                   interop: nil, ifd1: nil, thumbnailJPEG: nil)
            let makerNote = AppleMakerNote.build(burstUUID: burstUUID,
                                                 imageUniqueID: imageUniqueID, byteOrder: order)
            let newTIFF = TIFFBuilder.build(parsed: empty, makerNote: makerNote,
                                            pixelWidth: pixelWidth, pixelHeight: pixelHeight)
            var newPayload = Data("Exif".utf8)
            newPayload.append(contentsOf: [0, 0])
            newPayload.append(newTIFF)
            let newLen = newPayload.count + 2
            var rebuilt = Data()
            rebuilt.append(contentsOf: [0xff, 0xd8])
            rebuilt.append(contentsOf: [0xff, 0xe1, UInt8((newLen >> 8) & 0xff), UInt8(newLen & 0xff)])
            rebuilt.append(newPayload)
            // out 目前以 SOI 开头，去掉它再拼接。
            rebuilt.append(out.dropFirst(2))
            return rebuilt
        }
        return out
    }

    /// 校验用：从 JPEG 里读回 BurstUUID / ImageUniqueID。
    static func readBurstInfo(data: Data) -> (burstUUID: String?, imageUniqueID: String?) {
        guard data.count >= 4, data[0] == 0xff, data[1] == 0xd8 else { return (nil, nil) }
        var pos = 2
        while pos + 4 <= data.count, data[pos] == 0xff {
            let marker = data[pos + 1]
            if marker == 0xda || marker == 0xd9 { break }
            if marker == 0x01 || (0xd0...0xd7).contains(marker) { pos += 2; continue }
            let len = (Int(data[pos + 2]) << 8) | Int(data[pos + 3])
            guard len >= 2, pos + 2 + len <= data.count else { break }
            if marker == 0xe1, len >= 8 {
                let ps = pos + 4
                if data[ps] == 0x45 && data[ps + 1] == 0x78 && data[ps + 2] == 0x69
                    && data[ps + 3] == 0x66 && data[ps + 4] == 0 && data[ps + 5] == 0 {
                    let tiffData = data[(ps + 6)..<(pos + 2 + len)]
                    guard let parsed = try? TIFFParser.parse(tiffData),
                          let exif = parsed.exif,
                          let mn = exif.first(where: { $0.tag == 0x927c }) else { return (nil, nil) }
                    return AppleMakerNote.parse(mn.value, byteOrder: parsed.byteOrder)
                }
            }
            pos += 2 + len
        }
        return (nil, nil)
    }
}
