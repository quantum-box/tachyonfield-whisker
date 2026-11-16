// 見えない部品。アプリの根元に 1 つ置くと、置かれた画面（ウィンドウ）全体で
// iOS 標準に近いキーボードの閉じ方をそろえる。
//
// - 入力欄の外をタップすると閉じる（ボタン・行の選択などはそのまま届く）
// - 下へドラッグして離すと閉じる。スクロールする場所では、指に合わせてキーボードを引き下ろせる
// - 入力中の文字や選択はそのまま残る（閉じるだけ）。入力欄をもう一度タップすれば出る
//
// `FieldKeyboardKeepView` を置いた範囲のタップでは閉じない（チャットの送るボタンなど）。

#if canImport(UIKit)
import UIKit
import WhiskerModule

extension Notification.Name {
    /// 指でキーボードを引き下ろしている間の高さ（画面の下端から）。`userInfo["height"]`。
    static let fieldKeyboardTracking = Notification.Name("FieldKeyboardTracking")
}

@objc(FieldKeyboardDismissView)
public final class FieldKeyboardDismissView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        return view
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if let window { KeyboardDismisser.install(on: window) }
    }
}

/// タップで閉じない範囲の印。親の大きさいっぱいに置く。
@objc(FieldKeyboardKeepView)
public final class FieldKeyboardKeepView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        return view
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        // 範囲の印なので、触れたものは上や下の部品へ通す。
        isUserInteractionEnabled = false
        if window != nil { KeyboardDismisser.keepAreas.add(self) }
    }
}

/// ウィンドウごとに 1 つ。タップとドラッグを見張るだけで、触れたものは他の部品にもそのまま届ける。
final class KeyboardDismisser: NSObject, UIGestureRecognizerDelegate {
    static let keepAreas = NSHashTable<UIView>.weakObjects()
    private static let installed = NSMapTable<UIWindow, KeyboardDismisser>.weakToStrongObjects()

    /// 下へこれだけ動かして離したら閉じる（pt）。
    private static let dismissDistance: CGFloat = 40

    private weak var window: UIWindow?
    private let tap = UITapGestureRecognizer()
    private let pan = UIPanGestureRecognizer()
    private let tracker = TrackerView()
    private var keyboardShown = false
    private var observers: [NSObjectProtocol] = []

    static func install(on window: UIWindow) {
        guard installed.object(forKey: window) == nil else { return }
        installed.setObject(KeyboardDismisser(window: window), forKey: window)
    }

    private init(window: UIWindow) {
        self.window = window
        super.init()
        for recognizer in [tap, pan] as [UIGestureRecognizer] {
            recognizer.delegate = self
            recognizer.cancelsTouchesInView = false
            recognizer.delaysTouchesBegan = false
            recognizer.delaysTouchesEnded = false
            window.addGestureRecognizer(recognizer)
        }
        tap.addTarget(self, action: #selector(didTap))
        pan.addTarget(self, action: #selector(didPan))
        installTracker(in: window)

        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: UIResponder.keyboardWillShowNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.keyboardShown = true
            self?.makeScrollViewsInteractive()
        })
        observers.append(center.addObserver(
            forName: UIResponder.keyboardWillHideNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.keyboardShown = false
        })
    }

    deinit {
        observers.forEach { NotificationCenter.default.removeObserver($0) }
    }

    /// キーボードの上端に合わせて高さが変わる見えない板。指で引き下ろしている間の位置を測る。
    private func installTracker(in window: UIWindow) {
        // 隠すと高さの変化を受け取れないことがあるので、透明にして置く。
        tracker.alpha = 0
        tracker.isUserInteractionEnabled = false
        tracker.translatesAutoresizingMaskIntoConstraints = false
        window.addSubview(tracker)
        NSLayoutConstraint.activate([
            tracker.leadingAnchor.constraint(equalTo: window.leadingAnchor),
            tracker.widthAnchor.constraint(equalToConstant: 1),
            tracker.topAnchor.constraint(equalTo: window.keyboardLayoutGuide.topAnchor),
            tracker.bottomAnchor.constraint(equalTo: window.bottomAnchor),
        ])
        tracker.onResize = { [weak self] in self?.reportTracking() }
    }

    private var dragging: Bool {
        pan.state == .began || pan.state == .changed
    }

    private func reportTracking() {
        if keyboardShown, dragging { postTracking() }
    }

    private func postTracking() {
        guard let window else { return }
        var height = tracker.bounds.height
        // キーボードが無いときの案内線は、画面下の安全な余白の上にある。
        if height <= window.safeAreaInsets.bottom + 1 { height = 0 }
        NotificationCenter.default.post(name: .fieldKeyboardTracking, object: window, userInfo: ["height": height])
    }

    /// 今の画面のスクロールに、指でキーボードを引き下ろす動きを付ける。
    /// 部品側で決めてあるもの（検索付きの一覧の `.onDrag` など）と、複数行の入力欄はそのまま。
    private func makeScrollViewsInteractive() {
        guard let window else { return }
        var stack: [UIView] = [window]
        while let view = stack.popLast() {
            if let scroll = view as? UIScrollView, !(scroll is UITextView), scroll.keyboardDismissMode == .none {
                scroll.keyboardDismissMode = .interactive
            }
            stack.append(contentsOf: view.subviews)
        }
    }

    private func dismiss() {
        // 同じタップで動くもの（編集メニューの「ペースト」など）を先に済ませてから閉じる。
        DispatchQueue.main.async { [weak self] in self?.window?.endEditing(true) }
    }

    @objc private func didTap() {
        if tap.state == .ended { dismiss() }
    }

    @objc private func didPan() {
        guard pan.state == .ended || pan.state == .cancelled, let view = pan.view else { return }
        let moved = pan.translation(in: view)
        let velocity = pan.velocity(in: view)
        if pan.state == .ended, keyboardShown,
           moved.y > Self.dismissDistance, moved.y > abs(moved.x), velocity.y > -100 {
            dismiss()
        } else if keyboardShown {
            // 引き下ろしをやめて戻ったときは、戻り終えた位置を知らせ直す。
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self, self.keyboardShown, !self.dragging else { return }
                self.postTracking()
            }
        }
    }

    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard keyboardShown, let window else { return false }
        var view = touch.view
        while let current = view {
            // 入力欄そのもの（カーソル移動・文字の選択・ほかの欄への移動）は邪魔しない。
            if current is UITextView { return false }
            if recognizer === tap, current is UITextInput { return false }
            view = current.superview
        }
        if recognizer === tap {
            let point = touch.location(in: window)
            for keep in Self.keepAreas.allObjects where keep.window === window && !keep.isHidden {
                if keep.convert(keep.bounds, to: window).contains(point) { return false }
            }
        }
        return true
    }

    func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        guard recognizer === pan else { return true }
        let v = pan.velocity(in: pan.view)
        return abs(v.y) > abs(v.x)
    }

    func gestureRecognizer(
        _ recognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
    ) -> Bool {
        true
    }
}

private final class TrackerView: UIView {
    var onResize: (() -> Void)?
    private var lastHeight: CGFloat = -1

    override func layoutSubviews() {
        super.layoutSubviews()
        guard abs(bounds.height - lastHeight) > 0.5 else { return }
        lastHeight = bounds.height
        onResize?()
    }
}
#endif
