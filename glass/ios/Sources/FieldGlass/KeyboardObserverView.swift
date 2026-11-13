// 見えない部品。キーボードが出る・引っ込むたびに、画面の下端から測った高さを
// "change" { height } で Rust に知らせる。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldKeyboardObserverView)
public final class FieldKeyboardObserverView: WhiskerUI<UIView> {
    private var observer: NSObjectProtocol?

    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        observer = NotificationCenter.default.addObserver(
            forName: UIResponder.keyboardWillChangeFrameNotification, object: nil, queue: .main
        ) { [weak self] note in
            self?.keyboardWillChange(note)
        }
        return view
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    private func keyboardWillChange(_ note: Notification) {
        guard let frame = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
        let screen = window?.windowScene?.screen.bounds ?? UIScreen.main.bounds
        let height = max(0, screen.maxY - frame.minY)
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["height": Double(height)])
    }
}
#endif
