// 見えない部品。画面が明るい表示（ライト）か暗い表示（ダーク）かを、
// 置かれたときと切り替わるたびに "change" { dark } で Rust に知らせる。
// Rust 側で描く部分（帯・詳細画面など）の色を、iPhone の設定に合わせるために使う。
// あわせて、画面（ウィンドウ）の幅と、iPad の広い表示（横の大きさが regular）かを
// "layout" { wide, width } で知らせる。iPad では横のメニューを出しっぱなしにし、広ければ 2 列に並べる。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldAppearanceObserverView)
public final class FieldAppearanceObserverView: WhiskerUI<UIView> {
    private var last: Bool?
    private var lastLayout: (Bool, Int)?

    @objc public override func createView() -> UIView {
        let view = TraitWatcher()
        view.isUserInteractionEnabled = false
        view.onChange = { [weak self] dark in self?.report(dark) }
        view.onLayout = { [weak self] wide, width in self?.reportLayout(wide, width) }
        return view
    }

    private func reportLayout(_ wide: Bool, _ width: CGFloat) {
        let rounded = Int(width.rounded())
        if let lastLayout, lastLayout.0 == wide, lastLayout.1 == rounded { return }
        lastLayout = (wide, rounded)
        WhiskerCustomEvent.dispatch(
            from: self, name: "layout", params: ["wide": wide ? 1.0 : 0.0, "width": Double(rounded)])
    }

    private func report(_ dark: Bool) {
        guard dark != last else { return }
        last = dark
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["dark": dark ? 1.0 : 0.0])
    }
}

private final class TraitWatcher: UIView {
    var onChange: ((Bool) -> Void)?
    var onLayout: ((Bool, CGFloat) -> Void)?
    private var activeObserver: NSObjectProtocol?
    private var rotateObserver: NSObjectProtocol?

    override init(frame: CGRect) {
        super.init(frame: frame)
        // 起動の途中は、まだ明るい表示と答えることがある。アプリが前に出たときにも測り直す
        // （同じ答えなら Rust には知らせない）。
        activeObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.notify()
        }
        // 回すと幅が変わる（この部品自身は大きさ 0 なので、並べ直しでは気づけない）。回り終えてから測る。
        rotateObserver = NotificationCenter.default.addObserver(
            forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { self?.notify() }
        }
        if #available(iOS 17.0, *) {
            registerForTraitChanges([UITraitUserInterfaceStyle.self, UITraitHorizontalSizeClass.self]) { (view: TraitWatcher, _) in
                view.notify()
            }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    deinit {
        if let activeObserver { NotificationCenter.default.removeObserver(activeObserver) }
        if let rotateObserver { NotificationCenter.default.removeObserver(rotateObserver) }
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
        if let window {
            // iPad を全画面か広めの分割で使っているとき。iPhone や狭い分割（compact）は今までどおり。
            let regular = window.traitCollection.horizontalSizeClass == .regular
            onLayout?(regular && window.bounds.width >= 700, window.bounds.width)
        }
        // 自分の見た目ではなく、画面（ウィンドウ）の設定を見る。
        let style = window?.traitCollection.userInterfaceStyle ?? traitCollection.userInterfaceStyle
        guard style != .unspecified else { return }
        onChange?(style == .dark)
    }
}
#endif
