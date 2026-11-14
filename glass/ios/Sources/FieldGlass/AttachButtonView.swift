// Agent にファイルを渡すための「＋」ボタン。押すと「写真」「カメラ」「ファイル」を選ぶメニューが出る。
// 選ばれたものは一時フォルダに書き出し、その場所を Rust に知らせる（中身の読み込みと送信は Rust 側）。
// 写真は送る前に JPEG にして小さくする（HEIC はそのままだと Agent が読めないので）。
//
// Rust へ知らせるもの：
// - "pick" { path, name, type, size } … 選ばれたファイル 1 つごとに 1 回

#if canImport(UIKit)
import PhotosUI
import UIKit
import UniformTypeIdentifiers
import WhiskerModule

/// 写真を送る大きさの目安（Agent が画像として読める大きさに収める）。
private let imageBudget = 600_000
private let imageMaxSide: CGFloat = 2048

private final class Picker: NSObject, PHPickerViewControllerDelegate, UIDocumentPickerDelegate,
    UIImagePickerControllerDelegate, UINavigationControllerDelegate
{
    var onPick: ((URL, String, String) -> Void)?

    func presentMenu(from view: UIView, kind: String) {
        guard let top = topController(from: view) else { return }
        switch kind {
        case "photo":
            var config = PHPickerConfiguration()
            config.filter = .images
            config.selectionLimit = 10
            let picker = PHPickerViewController(configuration: config)
            picker.delegate = self
            top.present(picker, animated: true)
        case "camera":
            guard UIImagePickerController.isSourceTypeAvailable(.camera) else { return }
            let picker = UIImagePickerController()
            picker.sourceType = .camera
            picker.delegate = self
            top.present(picker, animated: true)
        default:
            let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: true)
            picker.allowsMultipleSelection = true
            picker.delegate = self
            top.present(picker, animated: true)
        }
    }

    private func topController(from view: UIView) -> UIViewController? {
        var top = view.window?.rootViewController
        while let presented = top?.presentedViewController { top = presented }
        return top
    }

    // 写真ライブラリ
    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        picker.dismiss(animated: true)
        for (index, result) in results.enumerated() {
            let provider = result.itemProvider
            guard provider.canLoadObject(ofClass: UIImage.self) else { continue }
            let base = (provider.suggestedName ?? "photo-\(index + 1)")
            provider.loadObject(ofClass: UIImage.self) { [weak self] object, _ in
                guard let image = object as? UIImage else { return }
                DispatchQueue.main.async { self?.deliver(image: image, name: base) }
            }
        }
    }

    // カメラ
    func imagePickerController(
        _ picker: UIImagePickerController,
        didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
    ) {
        picker.dismiss(animated: true)
        if let image = info[.originalImage] as? UIImage {
            let stamp = Int(Date().timeIntervalSince1970)
            deliver(image: image, name: "camera-\(stamp)")
        }
    }

    func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
        picker.dismiss(animated: true)
    }

    // ファイル
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        for url in urls {
            let name = url.lastPathComponent
            let type = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType
                ?? "application/octet-stream"
            let target = temporaryURL(ext: url.pathExtension)
            do {
                try FileManager.default.copyItem(at: url, to: target)
                onPick?(target, name, type)
            } catch {
                continue
            }
        }
    }

    private func deliver(image: UIImage, name: String) {
        guard let data = jpeg(image) else { return }
        let target = temporaryURL(ext: "jpg")
        do {
            try data.write(to: target)
        } catch {
            return
        }
        let stem = (name as NSString).deletingPathExtension
        onPick?(target, "\(stem).jpg", "image/jpeg")
    }

    /// 長い辺を 2048pt までにして、600KB に収まるまで画質を下げる。
    private func jpeg(_ image: UIImage) -> Data? {
        let side = max(image.size.width, image.size.height)
        var scaled = image
        if side > imageMaxSide {
            let ratio = imageMaxSide / side
            let size = CGSize(width: image.size.width * ratio, height: image.size.height * ratio)
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            scaled = UIGraphicsImageRenderer(size: size, format: format).image { _ in
                image.draw(in: CGRect(origin: .zero, size: size))
            }
        }
        var quality: CGFloat = 0.8
        var data = scaled.jpegData(compressionQuality: quality)
        while let current = data, current.count > imageBudget, quality > 0.3 {
            quality -= 0.15
            data = scaled.jpegData(compressionQuality: quality)
        }
        return data
    }

    private func temporaryURL(ext: String) -> URL {
        let name = "agent-\(UUID().uuidString)" + (ext.isEmpty ? "" : ".\(ext)")
        return FileManager.default.temporaryDirectory.appendingPathComponent(name)
    }
}

@objc(FieldAttachButtonView)
public final class FieldAttachButtonView: WhiskerUI<UIView> {
    private let picker = Picker()

    private lazy var button: UIButton = {
        var config = UIButton.Configuration.plain()
        config.image = UIImage(
            systemName: "plus",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 18, weight: .semibold))
        config.baseForegroundColor = .label
        let button = UIButton(configuration: config)
        button.showsMenuAsPrimaryAction = true
        button.menu = UIMenu(children: [
            UIAction(title: "写真", image: UIImage(systemName: "photo.on.rectangle")) { [weak self] _ in
                self?.open("photo")
            },
            UIAction(title: "カメラ", image: UIImage(systemName: "camera")) { [weak self] _ in
                self?.open("camera")
            },
            UIAction(title: "ファイル", image: UIImage(systemName: "folder")) { [weak self] _ in
                self?.open("file")
            },
        ])
        picker.onPick = { [weak self] url, name, type in
            guard let self else { return }
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?
                .doubleValue ?? 0
            WhiskerCustomEvent.dispatch(
                from: self, name: "pick",
                params: ["path": url.path, "name": name, "type": type, "size": size])
        }
        return button
    }()

    @objc public override func createView() -> UIView {
        button
    }

    private func open(_ kind: String) {
        picker.presentMenu(from: button, kind: kind)
    }
}
#endif
