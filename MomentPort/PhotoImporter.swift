import Foundation
import Photos

/// 导入到系统相册并回查验证。
///
/// 关键事实（不要绕过它）：
/// - PhotoKit 没有任何公开 API 可以直接设置 burstIdentifier；
///   burstIdentifier、representsBurst 都是只读属性。
/// - 因此本程序的导入方式是“按正常流程导入转换后的文件，
///   再用 PhotoKit 把刚导入的资产读回来，看相册到底给了什么”。
/// - 如果回查到所有帧共享同一个非空 burstIdentifier，且其中一帧
///   representsBurst 为 true，才算“被识别为连拍”；否则如实报告失败，
///   不把“已导入”说成“已成组”。
///
/// 导入顺序：关键帧先导入，其余按拍摄顺序导入。
/// 这只是尽力让相册把关键帧当作代表图，苹果没有承诺导入顺序
/// 决定代表图；导入后用户仍可在相册里打开连拍、点“选出最佳”改代表图。
enum PhotoImporter {
    enum ImportError: Error, LocalizedError {
        case notAuthorized
        case noFiles
        case performFailed(String)

        var errorDescription: String? {
            switch self {
            case .notAuthorized: return "未获得照片图库访问权限，请在系统设置里允许本程序访问照片。"
            case .noFiles: return "没有可导入的文件"
            case .performFailed(let m): return "导入失败：\(m)"
            }
        }
    }

    static func requestAuthorization(completion: @escaping (Bool) -> Void) {
        PHPhotoLibrary.requestAuthorization { status in
            DispatchQueue.main.async { completion(status == .authorized) }
        }
    }

    /// 导入一组转换结果，完成后回查。completion 在主线程回调。
    static func importAndVerify(result: MomentPortEngine.Result,
                                completion: @escaping (Swift.Result<ImportVerification, Error>) -> Void) {
        requestAuthorization { ok in
            guard ok else { completion(.failure(ImportError.notAuthorized)); return }
            // 关键帧先导：把转换时记录的关键帧文件排到第一位。
            // 这只是尽力而为，苹果没有承诺导入顺序决定代表图。
            var ordered = result.outputFiles
            if let key = result.keyOutputFile,
               let idx = ordered.firstIndex(of: key), idx != 0 {
                ordered.remove(at: idx)
                ordered.insert(key, at: 0)
            }
            guard !ordered.isEmpty else { completion(.failure(ImportError.noFiles)); return }

            var placeholders: [PHObjectPlaceholder] = []
            PHPhotoLibrary.shared().performChanges({
                for url in ordered {
                    let req = PHAssetCreationRequest.forAsset()
                    let options = PHAssetResourceCreationOptions()
                    options.shouldMoveFile = false
                    req.addResource(with: .photo, fileURL: url, options: options)
                    if let ph = req.placeholderForCreatedAsset {
                        placeholders.append(ph)
                    }
                }
            }, completionHandler: { success, error in
                DispatchQueue.main.async {
                    guard success else {
                        completion(.failure(ImportError.performFailed(
                            error?.localizedDescription ?? "未知错误")))
                        return
                    }
                    let ids = placeholders.map { $0.localIdentifier }
                    let assets = PHAsset.fetchAssets(withLocalIdentifiers: ids, options: nil)
                    var burstIDs: [String] = []
                    var represents = false
                    assets.enumerateObjects { asset, _, _ in
                        if let bid = asset.burstIdentifier, !bid.isEmpty {
                            burstIDs.append(bid)
                        }
                        if asset.representsBurst { represents = true }
                    }
                    let unique = Array(Set(burstIDs))
                    let grouped = !unique.isEmpty && unique.count == 1
                        && assets.count == ordered.count
                    let message: String
                    if grouped && represents {
                        message = "已识别为连拍：\(assets.count) 帧共享同一个 burstIdentifier，且有代表图。"
                    } else if grouped {
                        message = "已导入且共享 burstIdentifier，但未检测到代表图（representsBurst 为 false），请在相册里打开确认。"
                    } else if burstIDs.isEmpty {
                        message = "已导入 \(assets.count) 张，但相册没有给它们 burstIdentifier：本次导入未被识别为连拍。请不要删除输出文件夹，可改用相册里手动建相簿的方式浏览，或换一个相册版本再试（见 README 的“如果没成组”一节）。"
                    } else {
                        message = "已导入，但 burstIdentifier 不一致（共 \(unique.count) 个不同值），未成一组。"
                    }
                    completion(.success(ImportVerification(
                        burstUUID: result.burstUUID, frameCount: result.outputFiles.count,
                        importedCount: assets.count, burstIdentifiers: unique,
                        representsBurst: represents, message: message)))
                }
            })
        }
    }
}
