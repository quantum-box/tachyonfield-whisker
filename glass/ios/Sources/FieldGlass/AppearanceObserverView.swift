// 見えない部品。画面が明るい表示（ライト）か暗い表示（ダーク）かを、
// 置かれたときと切り替わるたびに "change" { dark } で Rust に知らせる。
// Rust 側で描く部分（帯・詳細画面など）の色を、iPhone の設定に合わせるために使う。
// あわせて、画面（ウィンドウ）の幅と、iPad の広い表示（横の大きさが regular）かを
// "layout" { wide, width } で知らせる。iPad では横のメニューを出しっぱなしにし、広ければ 2 列に並べる。
// 設定の「視差効果を減らす」（動きを減らす）が入っているかも "motion" { reduce } で知らせる。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldAppearanceObserverView)
public final class FieldAppearanceObserverView: WhiskerUI<UIView> {
    private var last: Bool?
    private var lastLayout: (Bool, Int)?
    private var lastMotion: Bool?

    @objc public override func createView() -> UIView {
        let view = TraitWatcher()
        view.isUserInteractionEnabled = false
        view.onChange = { [weak self] dark, force in self?.report(dark, force: force) }
        view.onLayout = { [weak self] wide, width, force in self?.reportLayout(wide, width, force: force) }
        view.onMotion = { [weak self] reduce, force in self?.reportMotion(reduce, force: force) }
        return view
    }

    private func reportLayout(_ wide: Bool, _ width: CGFloat, force: Bool) {
        let rounded = Int(width.rounded())
        if !force, let lastLayout, lastLayout.0 == wide, lastLayout.1 == rounded { return }
        lastLayout = (wide, rounded)
        WhiskerCustomEvent.dispatch(
            from: self, name: "layout", params: ["wide": wide ? 1.0 : 0.0, "width": Double(rounded)])
    }

    private func reportMotion(_ reduce: Bool, force: Bool) {
        guard force || reduce != lastMotion else { return }
        lastMotion = reduce
        WhiskerCustomEvent.dispatch(from: self, name: "motion", params: ["reduce": reduce ? 1.0 : 0.0])
    }

    private func report(_ dark: Bool, force: Bool) {
        guard force || dark != last else { return }
        last = dark
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["dark": dark ? 1.0 : 0.0])
    }
}

private final class TraitWatcher: UIView {
    var onChange: ((Bool, Bool) -> Void)?
    var onLayout: ((Bool, CGFloat, Bool) -> Void)?
    var onMotion: ((Bool, Bool) -> Void)?
    private var activeObserver: NSObjectProtocol?
    private var rotateObserver: NSObjectProtocol?
    private var motionObserver: NSObjectProtocol?
    /// ウィンドウの大きさの変化（Stage Manager や分割表示で幅を変えたとき）。
    private var geometryObservation: NSKeyValueObservation?

    override init(frame: CGRect) {
        super.init(frame: frame)
        // 起動中や復帰中は外観とランタイムの状態が変わるため、前に出た後に測り直す。
        // 同じ値の再送は Rust 側で無視する。
        activeObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in
            // Whisker resumes in its own didBecomeActive observer. Wait until
            // all observers have run before sending an event to its runtime.
            // Force a resend: a trait callback may have run while the runtime
            // was still paused even though UIApplication already said active.
            DispatchQueue.main.async { [weak self] in self?.notify(force: true) }
        }
        // 回すと幅が変わる（この部品自身は大きさ 0 なので、並べ直しでは気づけない）。回り終えてから測る。
        rotateObserver = NotificationCenter.default.addObserver(
            forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { self?.notify() }
        }
        motionObserver = NotificationCenter.default.addObserver(
            forName: UIAccessibility.reduceMotionStatusDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.notify()
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
        if let motionObserver { NotificationCenter.default.removeObserver(motionObserver) }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if window != nil { notify() }
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        geometryObservation = nil
        if #available(iOS 16.0, *), let scene = window?.windowScene {
            geometryObservation = scene.observe(\.effectiveGeometry, options: [.new]) { [weak self] _, _ in
                DispatchQueue.main.async { self?.notify() }
            }
        }
        if window != nil {
            DispatchQueue.main.async { [weak self] in self?.notify(force: true) }
        }
    }

    override func traitCollectionDidChange(_ previous: UITraitCollection?) {
        super.traitCollectionDidChange(previous)
        if #unavailable(iOS 17.0) { notify() }
    }

    private func notify(force: Bool = false) {
        // Whisker cannot receive events while paused. Re-sample after resume.
        guard UIApplication.shared.applicationState == .active, let window else { return }
        let regular = window.traitCollection.horizontalSizeClass == .regular
        onLayout?(regular && window.bounds.width >= 700, window.bounds.width, force)
        onMotion?(UIAccessibility.isReduceMotionEnabled, force)
        // 自分の見た目ではなく、画面（ウィンドウ）の設定を見る。
        let style = window.traitCollection.userInterfaceStyle
        guard style != .unspecified else { return }
        onChange?(style == .dark, force)
    }
}
#endif
