// ガラスの板。中身は持たず、上に重ねた Whisker の部品の「下敷き」になる。
//
// iOS 26 以降は Apple の本物のガラス（UIGlassEffect）。それより前の iOS では
// 普通のすりガラス（UIBlurEffect）にする。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldGlassView)
public final class FieldGlassView: WhiskerUI<UIView> {
    private lazy var effectView: UIVisualEffectView = {
        let view: UIVisualEffectView
        if #available(iOS 26.0, *) {
            let glass = UIGlassEffect()
            glass.isInteractive = true
            view = UIVisualEffectView(effect: glass)
        } else {
            view = UIVisualEffectView(effect: UIBlurEffect(style: .systemThinMaterial))
        }
        view.clipsToBounds = true
        return view
    }()

    @objc public override func createView() -> UIView {
        effectView
    }

    func setCornerRadius(_ radius: CGFloat) {
        effectView.layer.cornerRadius = radius
        if #available(iOS 26.0, *) {
            effectView.cornerConfiguration = .corners(radius: .fixed(radius))
        }
    }
}
#endif
