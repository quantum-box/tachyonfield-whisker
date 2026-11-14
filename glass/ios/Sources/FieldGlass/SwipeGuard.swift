// 一覧の上で指を横に動かしたとき（裏のメニューを引き出すときなど）に、
// 指を離した行が「押された」ことにならないようにする。
//
// 一覧は縦にスクロールしないと、横に動かしたあとの指離れもタップとして扱ってしまう。
// 横向きの動きだけを見張り、その最中と直後は行を選ばせない。

#if canImport(UIKit)
import UIKit

final class SwipeGuard: NSObject, UIGestureRecognizerDelegate {
    private let pan = UIPanGestureRecognizer()
    private var lastSwipe: CFTimeInterval = 0

    func attach(to view: UIView) {
        pan.addTarget(self, action: #selector(didPan))
        pan.delegate = self
        // 触れたものはそのまま一覧や Rust 側にも届ける。見張るだけ。
        pan.cancelsTouchesInView = false
        pan.delaysTouchesBegan = false
        pan.delaysTouchesEnded = false
        view.addGestureRecognizer(pan)
    }

    /// 今、行を選んでよいか。横に動かしている最中と、離した直後は選ばない。
    var allowsSelection: Bool {
        switch pan.state {
        case .began, .changed: return false
        default: return CACurrentMediaTime() - lastSwipe > 0.35
        }
    }

    @objc private func didPan() {
        lastSwipe = CACurrentMediaTime()
    }

    func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        let v = pan.velocity(in: pan.view)
        return abs(v.x) > abs(v.y)
    }

    func gestureRecognizer(
        _ recognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
    ) -> Bool {
        true
    }
}
#endif
