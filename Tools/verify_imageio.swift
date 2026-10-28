// System ImageIO validation; does not import or access any Photos library.
// Compile: swiftc -module-cache-path /tmp/momentport-swift-module-cache Tools/verify_imageio.swift -o /tmp/verify-imageio
// Native: verify-imageio --native 样片/apple
// Converted: verify-imageio <original-root> <output-root>
import Foundation
import ImageIO
func require(_ ok: @autoclosure () -> Bool, _ message: String) {
    if !ok() { fputs("FAIL: \(message)\n", stderr); exit(1) }
}
func properties(_ url: URL) -> NSDictionary {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) else {
        fputs("FAIL: ImageIO cannot read \(url.lastPathComponent)\n", stderr); exit(1)
    }
    return props as NSDictionary
}
func pixels(_ url: URL) -> CFData {
    let source = CGImageSourceCreateWithURL(url as CFURL, nil)!
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
    return image.dataProvider!.data!
}
func jpegs(_ root: URL) -> [URL] {
    let items = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil)!
    return items.compactMap { $0 as? URL }.filter { ["jpg", "jpeg"].contains($0.pathExtension.lowercased()) }.sorted { $0.path < $1.path }
}
let args = Array(CommandLine.arguments.dropFirst())
require(args.count == 2, "expected --native folder OR original-root output-root")
let native = args[0] == "--native"
let output = URL(fileURLWithPath: args[1])
let files = jpegs(output)
require(!files.isEmpty, "no JPEGs")
var groups = [String: Set<String>]()
var photoIDs = Set<String>()
let originals = native ? [] : jpegs(URL(fileURLWithPath: args[0]))
for file in files {
    let p = properties(file)
    guard let apple = p[kCGImagePropertyMakerAppleDictionary] as? NSDictionary,
          let burst = apple["11"] as? String, !burst.isEmpty,
          let photo = apple["43"] as? String, !photo.isEmpty else {
        fputs("FAIL: ImageIO missing Apple BurstUUID/PhotoIdentifier: \(file.lastPathComponent)\n", stderr); exit(1)
    }
    require((apple["1"] as? NSNumber)?.intValue == 17, "MakerNote version")
    require(photoIDs.insert(photo).inserted, "duplicate PhotoIdentifier")
    groups[file.deletingLastPathComponent().path, default: []].insert(burst)
    if !native {
        let matches = originals.filter { $0.lastPathComponent == file.lastPathComponent }
        require(matches.count == 1, "source match must be unique")
        let original = matches[0]
        let before = properties(original)
        for key in [kCGImagePropertyTIFFDictionary, kCGImagePropertyExifDictionary,
                    kCGImagePropertyGPSDictionary, kCGImagePropertyOrientation,
                    kCGImagePropertyPixelWidth, kCGImagePropertyPixelHeight, kCGImagePropertyProfileName] {
            require(NSDictionary(dictionary: ["v": p[key] ?? NSNull()]).isEqual(to:
                ["v": before[key] ?? NSNull()]), "metadata changed: \(key) in \(file.lastPathComponent)")
        }
        require(CFEqual(pixels(original), pixels(file)), "decoded pixels changed")
    }
}
require(groups.values.allSatisfy { $0.count == 1 }, "group BurstUUID mismatch")
print("PASS: ImageIO \(files.count) frames / \(groups.count) groups; Apple version, BurstUUID, unique PhotoIdentifier" + (native ? "; native baseline" : "; TIFF/EXIF/GPS/orientation/profile and decoded pixels unchanged"))
print("File parsing only; Apple Photos grouping has NOT been tested by this tool.")
