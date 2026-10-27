#!/usr/bin/env python3
"""验证转换结果：不依赖 exiftool，只用标准库。

用法：
  python3 Tools/verify_burst.py 输出文件夹/Burst_01 [原图文件夹]

检查三件事：
1. 同一组每个文件的 MakerNote 里 BurstUUID 相同；
2. 每帧的 ImageUniqueID 各不相同；
3. 若给了原图文件夹：转换后文件的 SOS 扫描数据与原图完全相同
   （即像素没有被重新编码，只换了 EXIF 段）。

MakerNote 格式与 BurstConverter 的 AppleMakerNote.swift 一致：
迷你 TIFF IFD，字节序与外层 TIFF 相同，字符串偏移相对 MakerNote 起始。
"""
import os
import struct
import sys
from pathlib import Path

TYPE_SIZE = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2, 9: 4, 10: 8, 11: 4, 12: 8}


def parse_tiff(tiff):
    if tiff[:2] == b"II":
        endian = "<"
    elif tiff[:2] == b"MM":
        endian = ">"
    else:
        raise ValueError("unknown TIFF byte order")
    if struct.unpack_from(endian + "H", tiff, 2)[0] != 42:
        raise ValueError("bad TIFF magic")
    ifd0_off = struct.unpack_from(endian + "I", tiff, 4)[0]

    def parse_ifd(off):
        (count,) = struct.unpack_from(endian + "H", tiff, off)
        entries = {}
        for i in range(count):
            e = off + 2 + i * 12
            tag, typ, cnt = struct.unpack_from(endian + "HHI", tiff, e)
            size = cnt * TYPE_SIZE.get(typ, 1)
            if size <= 4:
                value = tiff[e + 8 : e + 8 + size]
            else:
                (voff,) = struct.unpack_from(endian + "I", tiff, e + 8)
                value = tiff[voff : voff + size]
            entries[tag] = (typ, cnt, value)
        (nxt,) = struct.unpack_from(endian + "I", tiff, off + 2 + count * 12)
        return entries, nxt

    ifd0, _ = parse_ifd(ifd0_off)
    exif = None
    if 0x8769 in ifd0:
        (ptr,) = struct.unpack(endian + "I", ifd0[0x8769][2][:4])
        exif, _ = parse_ifd(ptr)
    return endian, exif


def parse_makernote(data, endian):
    (count,) = struct.unpack_from(endian + "H", data, 0)
    out = {}
    for i in range(count):
        e = 2 + i * 12
        tag, typ, cnt = struct.unpack_from(endian + "HHI", data, e)
        size = cnt * TYPE_SIZE.get(typ, 1)
        if typ != 2:
            continue
        if size <= 4:
            raw = data[e + 8 : e + 8 + size]
        else:
            (voff,) = struct.unpack_from(endian + "I", data, e + 8)
            raw = data[voff : voff + size]
        out[tag] = raw.split(b"\x00")[0].decode("ascii", "replace")
    return out.get(0x000B), out.get(0x0015)


def burst_info(path):
    data = Path(path).read_bytes()
    if data[:2] != b"\xff\xd8":
        raise ValueError("not JPEG")
    pos = 2
    while pos + 4 <= len(data) and data[pos] == 0xFF:
        marker = data[pos + 1]
        if marker == 0xDA or marker == 0xD9:
            break
        if marker == 0x01 or 0xD0 <= marker <= 0xD7:
            pos += 2
            continue
        length = struct.unpack_from(">H", data, pos + 2)[0]
        if marker == 0xE1 and length >= 8 and data[pos + 4 : pos + 10] == b"Exif\x00\x00":
            tiff = data[pos + 10 : pos + 2 + length]
            endian, exif = parse_tiff(tiff)
            if exif and 0x927C in exif:
                return parse_makernote(exif[0x927C][2], endian)
            return None, None
        pos += 2 + length
    return None, None


def sos_suffix(path):
    data = Path(path).read_bytes()
    pos = 2
    while pos + 4 <= len(data) and data[pos] == 0xFF:
        marker = data[pos + 1]
        if marker == 0xDA:
            return data[pos:]
        if marker == 0x01 or 0xD0 <= marker <= 0xD7:
            pos += 2
            continue
        length = struct.unpack_from(">H", data, pos + 2)[0]
        pos += 2 + length
    return None


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    folder = Path(sys.argv[1])
    original_folder = Path(sys.argv[2]) if len(sys.argv) > 2 else None
    files = sorted(p for p in folder.iterdir() if p.suffix.lower() in (".jpg", ".jpeg"))
    if not files:
        print("文件夹里没有 JPEG")
        return 1
    bursts = set()
    uniques = set()
    ok = True
    for f in files:
        burst, unique = burst_info(f)
        print(f"{f.name}: BurstUUID={burst} ImageUniqueID={unique}")
        if burst is None:
            print("  !! 读不到 BurstUUID")
            ok = False
        else:
            bursts.add(burst)
        if unique:
            if unique in uniques:
                print("  !! ImageUniqueID 重复")
                ok = False
            uniques.add(unique)
        if original_folder is not None:
            orig = original_folder / f.name
            # 重名加帧序号的情况：按同名找不到就跳过像素比对，只报提示。
            if orig.exists():
                if sos_suffix(f) != sos_suffix(orig):
                    print("  !! SOS 扫描数据与原图不一致（像素可能被改动）")
                    ok = False
                else:
                    print("  像素扫描数据与原图一致")
    print(f"共 {len(files)} 帧，BurstUUID 种类：{len(bursts)}（应为 1），ImageUniqueID 种类：{len(uniques)}（应为 {len(files)}）")
    if len(bursts) == 1 and len(uniques) == len(files) and ok:
        print("通过：同一组共享 BurstUUID，每帧唯一，文件层分组标识完整。")
        print("注意：这只证明文件写对了，最终是否在相册里成组，还要看导入后的 PhotoKit 回查。")
        return 0
    print("未通过：见上面的 !! 标记。")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
