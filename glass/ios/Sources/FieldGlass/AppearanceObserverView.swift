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
    private var activeObserver: NSObjectProtocol?

    override init(frame: CGRect) {
        super.init(frame: frame)
        // 起動の途中は、まだ明るい表示と答えることがある。アプリが前に出たときにも測り直す
        // （同じ答えなら Rust には知らせない）。
        activeObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.notify()
        }
        if #available(iOS 17.0, *) {
            registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: TraitWatcher, _) in
                view.notify()
            }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    deinit {
        if let activeObserver { NotificationCenter.default.removeObserver(activeObserver) }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if window != nil { notify() }
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { notify() }
    }

    override func traitCollectionDidChange(_ previous: UITraitCollection?) {
        super.traitCollectionDidChange(previous)
        if #unavailable(iOS 17.0) { notify() }
    }

    private func notify() {
        // 自分の見た目ではなく、画面（ウィンドウ）の設定を見る。
        let style = window?.traitCollection.userInterfaceStyle ?? traitCollection.userInterfaceStyle
        guard style != .unspecified else { return }
        onChange?(style == .dark)
    }
}
#endif
