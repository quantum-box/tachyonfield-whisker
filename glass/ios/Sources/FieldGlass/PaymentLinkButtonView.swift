// 利用者が選んだときだけ、支払リンクを開く・コピー・共有する。
// Props: label / url。Event: done { ok, action: open | copy | share, error }。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldPaymentLinkButtonView)
public final class FieldPaymentLinkButtonView: WhiskerUI<UIView>, UIAdaptivePresentationControllerDelegate {
    private var target: URL?
    private var label = "支払リンク"
    private var busy = false
    private var sharing: UIActivityViewController?

    private lazy var button: UIButton = {
        var configuration = UIButton.Configuration.plain()
        configuration.title = label
        configuration.image = UIImage(systemName: "link")
        configuration.imagePadding = 6
        let value = UIButton(configuration: configuration)
        value.isEnabled = false
        value.showsMenuAsPrimaryAction = true
        return value
    }()

    @objc public override func createView() -> UIView { button }

    func setLabel(_ value: String) {
        label = value.isEmpty ? "支払リンク" : value
        refreshButton()
    }

    func setUrl(_ value: String) {
        let next = safeUrl(value)
        if next != target {
            cancelShare()
            busy = false
        }
        target = next
        refreshButton()
    }

    private func safeUrl(_ value: String) -> URL? {
        guard !value.isEmpty,
            !value.unicodeScalars.contains(where: {
                CharacterSet.whitespacesAndNewlines.contains($0) || CharacterSet.controlCharacters.contains($0)
            }),
            let components = URLComponents(string: value),
            components.scheme?.lowercased() == "https",
            let host = components.host, !host.isEmpty,
            components.user == nil, components.password == nil,
            let url = components.url
        else { return nil }
        return url
    }

    private func refreshButton() {
        button.configuration?.title = target == nil ? "支払リンクを確認できません" : label
        button.isEnabled = target != nil && !busy
        guard let url = target else {
            button.menu = nil
            return
        }
        // Capture the displayed URL. A later record update must not repurpose an
        // already open menu into an action on a different customer's link.
        button.menu = UIMenu(title: url.host ?? "", children: [
            UIAction(title: "支払リンクを開く", image: UIImage(systemName: "safari")) { [weak self] _ in
                self?.perform("open", url: url)
            },
            UIAction(title: "リンクをコピー", image: UIImage(systemName: "doc.on.doc")) { [weak self] _ in
                self?.perform("copy", url: url)
            },
            UIAction(title: "リンクを共有", image: UIImage(systemName: "square.and.arrow.up")) { [weak self] _ in
                self?.perform("share", url: url)
            },
        ])
    }

    private func perform(_ action: String, url: URL) {
        guard target == url, !busy else { return }
        switch action {
        case "copy":
            UIPasteboard.general.url = url
            notify(ok: true, action: action)
        case "open":
            busy = true
            button.isEnabled = false
            UIApplication.shared.open(url, options: [:]) { [weak self] opened in
                guard let self, self.target == url else { return }
                self.busy = false
                self.refreshButton()
                self.notify(ok: opened, action: action,
                    error: opened ? "" : "支払リンクを開けませんでした。リンクのコピーをお試しください。")
            }
        case "share":
            guard let top = topController(), !top.isBeingPresented, !top.isBeingDismissed else {
                notify(ok: false, action: action, error: "共有画面を開けませんでした。もう一度お試しください。")
                return
            }
            busy = true
            button.isEnabled = false
            let controller = UIActivityViewController(activityItems: [url], applicationActivities: nil)
            sharing = controller
            controller.completionWithItemsHandler = { [weak self, weak controller] _, completed, _, error in
                guard let self, let controller, self.target == url, self.sharing === controller else { return }
                self.sharing = nil
                self.busy = false
                self.refreshButton()
                self.notify(ok: completed && error == nil, action: action,
                    error: error == nil ? "" : "支払リンクの共有を完了できませんでした。")
            }
            if let popover = controller.popoverPresentationController {
                popover.sourceView = button
                popover.sourceRect = button.bounds
            }
            top.present(controller, animated: true)
            controller.presentationController?.delegate = self
        default:
            return
        }
    }

    public func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        guard sharing === presentationController.presentedViewController else { return }
        sharing = nil
        busy = false
        refreshButton()
        notify(ok: false, action: "share")
    }

    private func cancelShare() {
        let controller = sharing
        sharing = nil
        controller?.dismiss(animated: false)
    }

    private func topController() -> UIViewController? {
        var top = button.window?.rootViewController
        while let presented = top?.presentedViewController { top = presented }
        return top
    }

    private func notify(ok: Bool, action: String, error: String = "") {
        WhiskerCustomEvent.dispatch(from: self, name: "done",
            params: ["ok": ok, "action": action, "error": error])
    }

    deinit { cancelShare() }
}
#endif
