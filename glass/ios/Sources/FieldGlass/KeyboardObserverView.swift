// 見えない部品。キーボードが出る・引っ込むたびに、画面の下端から測った高さを
// "change" { height } で Rust に知らせる。指でキーボードを引き下ろしている間（`KeyboardDismiss` を
// 置いたとき）は、指に合わせて知らせ続ける。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldKeyboardObserverView)
public final class FieldKeyboardObserverView: WhiskerUI<UIView> {
    private var observer: NSObjectProtocol?
    private var trackingObserver: NSObjectProtocol?
    private var lastHeight: Double = -1

    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        observer = NotificationCenter.default.addObserver(
            forName: UIResponder.keyboardWillChangeFrameNotification, object: nil, queue: .main
        ) { [weak self] note in
            self?.keyboardWillChange(note)
        }
        trackingObserver = NotificationCenter.default.addObserver(
            forName: .fieldKeyboardTracking, object: nil, queue: .main
        ) { [weak self] note in
            guard let self, let source = note.object as? UIWindow, source === self.window,
                  let height = note.userInfo?["height"] as? CGFloat else { return }
            self.report(Double(height))
        }
        return view
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        if let trackingObserver { NotificationCenter.default.removeObserver(trackingObserver) }
    }

    private func keyboardWillChange(_ note: Notification) {
        guard let frame = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
        let screen = window?.windowScene?.screen.bounds ?? UIScreen.main.bounds
        let height = max(0, screen.maxY - frame.minY)
        report(Double(height))
    }

    private func report(_ height: Double) {
        guard abs(height - lastHeight) > 0.5 else { return }
        lastHeight = height
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["height": height])
    }
}
#endif
