// 手動タップでCSVをファイルへ保存・共有する。CSV取得や自動送信は行わない。
// Props: label / csv（UTF-8の文字列）/ name（ファイル名）。
// Event: done { ok, action: "save" | "share", name, error }。

#if canImport(UIKit)
import UIKit
import WhiskerModule

private final class CsvExporter: NSObject, UIDocumentPickerDelegate,
    UIAdaptivePresentationControllerDelegate
{
    var onDone: ((Bool, String, String, String) -> Void)?
    private var directory: URL?
    private var activeController: UIViewController?
    private var operation = ""
    private var filename = ""

    func present(csv: String, name: String, action: String, from view: UIView) {
        guard activeController == nil else { return }
        guard !csv.isEmpty, let top = topController(from: view),
            !top.isBeingPresented, !top.isBeingDismissed
        else {
            onDone?(false, action, name, "CSV保存画面を開けませんでした。もう一度お試しください。")
            return
        }
        operation = action
        filename = safeFilename(name)
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("field-csv-\(UUID().uuidString)", isDirectory: true)
        let file = folder.appendingPathComponent(filename, isDirectory: false)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            directory = folder
            // Preserve the server's quoting, line endings and formula protection.
            try Data(csv.utf8).write(to: file, options: [.atomic, .completeFileProtection])
        } catch {
            clearFile()
            onDone?(false, action, filename, "CSVファイルを準備できませんでした。空き容量を確認してください。")
            return
        }

        if action == "save" {
            let picker = UIDocumentPickerViewController(forExporting: [file], asCopy: true)
            picker.delegate = self
            picker.allowsMultipleSelection = false
            activeController = picker
            top.present(picker, animated: true)
            picker.presentationController?.delegate = self
        } else {
            let controller = UIActivityViewController(activityItems: [file], applicationActivities: nil)
            controller.completionWithItemsHandler = { [weak self, weak controller] _, completed, _, error in
                guard let controller else { return }
                self?.finish(controller: controller, ok: completed && error == nil,
                    error: error == nil ? "" : "CSVの共有を完了できませんでした。もう一度お試しください。")
            }
            // An iPad share sheet requires an anchor, including in a split view.
            if let popover = controller.popoverPresentationController {
                popover.sourceView = view
                popover.sourceRect = view.bounds
            }
            activeController = controller
            top.present(controller, animated: true)
            controller.presentationController?.delegate = self
        }
    }

    func cancel() {
        let controller = activeController
        // A prop update or unmount is not a user event. Avoid dispatching back
        // into the Rust renderer while it is replacing this native view.
        activeController = nil
        clearFile()
        controller?.dismiss(animated: false)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        finish(controller: controller, ok: !urls.isEmpty)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        finish(controller: controller, ok: false)
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        finish(controller: presentationController.presentedViewController, ok: false)
    }

    private func finish(controller: UIViewController, ok: Bool, error: String = "") {
        // A dismissed sheet may complete after another export has begun.
        // Only the sheet for the current file may finish or delete that file.
        guard activeController === controller else { return }
        activeController = nil
        clearFile()
        onDone?(ok, operation, filename, error)
    }

    private func clearFile() {
        if let directory { try? FileManager.default.removeItem(at: directory) }
        directory = nil
    }

    private func topController(from view: UIView) -> UIViewController? {
        var top = view.window?.rootViewController
        while let presented = top?.presentedViewController { top = presented }
        return top
    }

    private func safeFilename(_ name: String) -> String {
        let cleaned = name.unicodeScalars.filter {
            !CharacterSet.controlCharacters.contains($0)
                && $0 != "/" && $0 != "\\" && $0 != ":"
        }
        var value = String(String.UnicodeScalarView(cleaned)).trimmingCharacters(in: .whitespacesAndNewlines)
        if value.isEmpty || value.hasPrefix(".") { value = "reservations.csv" }
        if value.lowercased().hasSuffix(".csv") { value = String(value.dropLast(4)) }
        return String(value.prefix(120)) + ".csv"
    }

    deinit { clearFile() }
}

@objc(FieldCsvExportButtonView)
public final class FieldCsvExportButtonView: WhiskerUI<UIView> {
    private let exporter = CsvExporter()
    private var csv = ""
    private var filename = "reservations.csv"
    private var busy = false

    private lazy var button: UIButton = {
        var configuration = UIButton.Configuration.plain()
        configuration.title = "CSVを保存・共有"
        configuration.image = UIImage(systemName: "square.and.arrow.up")
        configuration.imagePadding = 6
        let value = UIButton(configuration: configuration)
        value.isEnabled = false
        value.showsMenuAsPrimaryAction = true
        value.menu = UIMenu(children: [
            UIAction(title: "ファイルに保存", image: UIImage(systemName: "folder")) { [weak self] _ in
                self?.open("save")
            },
            UIAction(title: "共有", image: UIImage(systemName: "square.and.arrow.up")) { [weak self] _ in
                self?.open("share")
            },
        ])
        exporter.onDone = { [weak self] ok, action, name, error in
            guard let self else { return }
            self.busy = false
            self.button.isEnabled = !self.csv.isEmpty
            WhiskerCustomEvent.dispatch(from: self, name: "done",
                params: ["ok": ok, "action": action, "name": name, "error": error])
        }
        return value
    }()

    @objc public override func createView() -> UIView { button }

    func setLabel(_ value: String) {
        button.configuration?.title = value.isEmpty ? "CSVを保存・共有" : value
    }

    func setCsv(_ value: String) {
        if value != csv {
            exporter.cancel()
            busy = false
        }
        csv = value
        button.isEnabled = !csv.isEmpty && !busy
    }

    func setName(_ value: String) {
        if value != filename {
            exporter.cancel()
            busy = false
        }
        filename = value.isEmpty ? "reservations.csv" : value
        button.isEnabled = !csv.isEmpty && !busy
    }

    private func open(_ action: String) {
        guard !csv.isEmpty, !busy else { return }
        busy = true
        button.isEnabled = false
        exporter.present(csv: csv, name: filename, action: action, from: button)
    }

    deinit { exporter.cancel() }
}
#endif
