// 下から出る iOS 標準のシート（UISheetPresentationController）。中身は設定アプリ風の一覧。
//
// この部品そのものは画面に何も描かない。`open` が true になると、今いちばん上にある画面から
// シートを出し、false になると閉じる。
// Rust から受け取るもの：
// - open     … true の間シートを出す
// - title    … シート上部の題名
// - sections … 一覧の JSON（形は `MenuListView.swift` と同じ）
// - search   … 検索欄の「例」の文字。空なら検索欄を出さない
// Rust へ知らせるもの：
// - "select"  { key } … 行が押された（閉じるかどうかは Rust が `open` で決める）
// - "dismiss" {}      … 下へ引っ張る・「閉じる」で、利用者がシートを閉じた

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// シートの中身：題名・検索欄・「閉じる」付きの一覧。検索はこの中だけで絞り込む。
private final class SheetListController: UITableViewController, UISearchResultsUpdating,
    UISearchBarDelegate
{
    let model = MenuTableModel()
    var onClose: (() -> Void)?
    private var allSections: [MenuSection] = []
    private let search = UISearchController(searchResultsController: nil)
    /// 検索欄の「例」の文字。空なら検索欄を出さない。
    var searchPlaceholder = "" {
        didSet { if isViewLoaded { applySearch() } }
    }

    /// Rust から届いた一覧を受け取り、今の検索語で絞り直す。
    func setSections(_ json: String) {
        model.setSections(json)
        allSections = model.sections
        applyFilter()
    }

    func updateSearchResults(for searchController: UISearchController) {
        applyFilter()
    }

    /// 検索欄を押したら、シートを全画面まで広げる（半分のままだと候補が見えにくい）。
    func searchBarTextDidBeginEditing(_ searchBar: UISearchBar) {
        guard let sheet = navigationController?.sheetPresentationController else { return }
        sheet.animateChanges { sheet.selectedDetentIdentifier = .large }
    }

    private func applyFilter() {
        let query = (search.searchBar.text ?? "").trimmingCharacters(in: .whitespaces)
        if query.isEmpty {
            model.sections = allSections
        } else {
            model.sections = allSections.compactMap { section in
                let rows = section.rows.filter {
                    $0.title.localizedCaseInsensitiveContains(query)
                        || $0.subtitle.localizedCaseInsensitiveContains(query)
                }
                return rows.isEmpty ? nil : MenuSection(title: section.title, rows: rows)
            }
        }
        if isViewLoaded { tableView.reloadData() }
    }

    /// 検索欄を出す・消す。消すときは打ちかけの語も消して、一覧を全部に戻す。
    private func applySearch() {
        if searchPlaceholder.isEmpty {
            search.searchBar.text = ""
            navigationItem.searchController = nil
            applyFilter()
        } else {
            search.searchBar.placeholder = searchPlaceholder
            navigationItem.searchController = search
        }
    }

    init() {
        super.init(style: .insetGrouped)
    }

    required init?(coder: NSCoder) { nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        model.attach(to: tableView)
        search.searchResultsUpdater = self
        search.obscuresBackgroundDuringPresentation = false
        search.hidesNavigationBarDuringPresentation = false
        search.searchBar.delegate = self
        navigationItem.hidesSearchBarWhenScrolling = false
        if #available(iOS 16.0, *) {
            navigationItem.preferredSearchBarPlacement = .stacked
        }
        definesPresentationContext = true
        applySearch()
        navigationItem.rightBarButtonItem = UIBarButtonItem(
            systemItem: .close,
            primaryAction: UIAction { [weak self] _ in self?.onClose?() })
    }
}

@objc(FieldSheetView)
public final class FieldSheetView: WhiskerUI<UIView>, UIAdaptivePresentationControllerDelegate {
    private var isOpen = false
    private var sheetTitle = ""
    private var sectionsJSON = "[]"
    private var searchPlaceholder = ""
    private weak var presented: UINavigationController?
    private weak var list: SheetListController?

    private lazy var host: SheetHost = {
        let host = SheetHost()
        host.onDetach = { [weak self] in self?.close(animated: false) }
        host.onAttach = { [weak self] in self?.sync() }
        return host
    }()

    @objc public override func createView() -> UIView {
        host
    }

    func setOpen(_ on: Bool) {
        isOpen = on
        sync()
    }

    func setTitle(_ text: String) {
        sheetTitle = text
        list?.title = text
    }

    func setSections(_ json: String) {
        sectionsJSON = json
        list?.setSections(json)
    }

    func setSearch(_ placeholder: String) {
        searchPlaceholder = placeholder
        list?.searchPlaceholder = placeholder
    }

    private func sync() {
        if isOpen { show() } else { close(animated: true) }
    }

    private func show() {
        guard presented == nil, host.window != nil, let top = Self.topController(from: host.window) else {
            return
        }
        let list = SheetListController()
        list.title = sheetTitle
        list.searchPlaceholder = searchPlaceholder
        list.setSections(sectionsJSON)
        list.model.onSelect = { [weak self] key in
            guard let self else { return }
            WhiskerCustomEvent.dispatch(from: self, name: "select", params: ["key": key])
        }
        list.onClose = { [weak self] in
            guard let self else { return }
            self.close(animated: true)
            WhiskerCustomEvent.dispatch(from: self, name: "dismiss", params: [:])
        }
        let nav = UINavigationController(rootViewController: list)
        if let sheet = nav.sheetPresentationController {
            sheet.detents = [.medium(), .large()]
            sheet.prefersGrabberVisible = true
        }
        nav.presentationController?.delegate = self
        self.list = list
        presented = nav
        top.present(nav, animated: true)
    }

    private func close(animated: Bool) {
        guard let nav = presented else { return }
        presented = nil
        list = nil
        nav.dismiss(animated: animated)
    }

    /// 利用者が下へ引っ張って閉じたとき。
    public func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        presented = nil
        list = nil
        WhiskerCustomEvent.dispatch(from: self, name: "dismiss", params: [:])
    }

    private static func topController(from window: UIWindow?) -> UIViewController? {
        var top = window?.rootViewController
        while let next = top?.presentedViewController { top = next }
        return top
    }
}

/// 画面に載った・外れたを知らせるだけの見えない箱。
private final class SheetHost: UIView {
    var onAttach: (() -> Void)?
    var onDetach: (() -> Void)?

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { onAttach?() } else { onDetach?() }
    }
}
#endif
