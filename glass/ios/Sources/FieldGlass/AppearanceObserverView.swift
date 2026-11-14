// 見えない部品。画面が明るい表示（ライト）か暗い表示（ダーク）かを、
// 置かれたときと切り替わるたびに "change" { dark } で Rust に知らせる。
// Rust 側で描く部分（帯・詳細画面など）の色を、iPhone の設定に合わせるために使う。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldAppearanceObserverView)
public final class FieldAppearanceObserverView: WhiskerUI<UIView> {
    private var last: Bool?

    @objc public override func createView() -> UIView {
        let view = TraitWatcher()
        view.isUserInteractionEnabled = false
        view.onChange = { [weak self] dark in self?.report(dark) }
        return view
    }

    private func report(_ dark: Bool) {
        guard dark != last else { return }
        last = dark
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["dark": dark ? 1.0 : 0.0])
    }
}

private final class TraitWatcher: UIView {
    var onChange: ((Bool) -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        if #available(iOS 17.0, *) {
            registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: TraitWatcher, _) in
                view.notify()
            }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { notify() }
    }

    override func traitCollectionDidChange(_ previous: UITraitCollection?) {
        super.traitCollectionDidChange(previous)
        if #unavailable(iOS 17.0) { notify() }
    }

    private func notify() {
        onChange?(traitCollection.userInterfaceStyle == .dark)
    }
}
#endif
