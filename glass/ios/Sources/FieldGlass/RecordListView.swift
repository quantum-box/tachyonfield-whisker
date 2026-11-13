// iOS 標準のデータ一覧（UITableView）。連絡先アプリのような見た目で、
// 上に標準の検索欄、引っ張ると再読み込みが付く。
//
// Rust から受け取るもの（どれも文字）：
// - rows        … 行の JSON：[{ "key", "title", "subtitle", "trailing", "avatar": bool }]
// - header      … 一覧の上の小見出し（例：「6 件」）。空なら出さない
// - message     … 行が無いときに真ん中に出す文（「読み込み中…」「見つかりません」など）
// - placeholder … 検索欄の薄い文字
// - refreshing  … "true" の間は引っ張り更新のくるくるを出す
// - bottom-inset… 下に浮かぶタブに隠れないよう、一覧の下にあける高さ（pt）
// Rust へ知らせるもの：
// - "select"  { key }   … 行が押された
// - "search"  { value } … 検索欄の文字が変わった
// - "refresh" {}        … 引っ張って再読み込みされた

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// 一覧を入れる箱。上に検索欄を固定し、その下で一覧だけが流れる（iOS の連絡先アプリと同じ動き）。
/// 一覧を少しでも下へ流したら、検索欄との境目に細い線を出す。
/// Whisker の入れ物に直接置くと UIKit の部品が中身を並べ直さないので、大きさが決まるたびに付け直す。
private final class RecordListHost: UIView {
    let searchBar: UISearchBar = {
        let bar = UISearchBar()
        bar.searchBarStyle = .minimal
        bar.autocapitalizationType = .none
        return bar
    }()
    let table = UITableView(frame: .zero, style: .plain)
    let edge: UIView = {
        let line = UIView()
        line.backgroundColor = .separator
        line.alpha = 0
        return line
    }()
    private var observation: NSKeyValueObservation?

    override init(frame: CGRect) {
        super.init(frame: frame)
        addSubview(table)
        addSubview(searchBar)
        addSubview(edge)
        observation = table.observe(\.contentOffset, options: [.new]) { [weak self] table, _ in
            let scrolled = table.contentOffset.y + table.adjustedContentInset.top > 1
            let alpha: CGFloat = scrolled ? 1 : 0
            guard let self, self.edge.alpha != alpha else { return }
            UIView.animate(withDuration: 0.15) { self.edge.alpha = alpha }
        }
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        let barHeight = searchBar.sizeThatFits(bounds.size).height
        searchBar.frame = CGRect(x: 0, y: 0, width: bounds.width, height: barHeight)
        edge.frame = CGRect(x: 0, y: barHeight, width: bounds.width, height: 1 / max(traitCollection.displayScale, 1))
        table.frame = CGRect(x: 0, y: barHeight, width: bounds.width, height: max(bounds.height - barHeight, 0))
        if let header = table.tableHeaderView, header.frame.width != bounds.width {
            header.frame.size.width = bounds.width
            table.tableHeaderView = header
        }
    }
}

private struct RecordRow {
    let key: String
    let title: String
    let subtitle: String
    let trailing: String
    let avatar: Bool
}

@objc(FieldRecordListView)
public final class FieldRecordListView: WhiskerUI<UIView>, UITableViewDataSource, UITableViewDelegate,
    UISearchBarDelegate
{
    private var rows: [RecordRow] = []
    private var header: String = ""

    private var searchBar: UISearchBar { host.searchBar }

    /// 件数（「6 件」）。一覧の先頭に置き、一覧と一緒に流れる（上に貼り付かない）。
    private let countLabel: UILabel = {
        let label = UILabel()
        label.font = .preferredFont(forTextStyle: .footnote)
        label.textColor = .secondaryLabel
        return label
    }()

    private lazy var countHeader: UIView = {
        let view = UIView(frame: CGRect(x: 0, y: 0, width: 320, height: 32))
        countLabel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(countLabel)
        NSLayoutConstraint.activate([
            countLabel.leadingAnchor.constraint(equalTo: view.layoutMarginsGuide.leadingAnchor),
            countLabel.trailingAnchor.constraint(equalTo: view.layoutMarginsGuide.trailingAnchor),
            countLabel.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -6),
        ])
        return view
    }()

    private let messageLabel: UILabel = {
        let label = UILabel()
        label.textColor = .secondaryLabel
        label.font = .preferredFont(forTextStyle: .body)
        label.textAlignment = .center
        label.numberOfLines = 0
        return label
    }()

    private let refresh = UIRefreshControl()

    private lazy var host: RecordListHost = {
        let host = RecordListHost()
        let table = host.table
        table.dataSource = self
        table.delegate = self
        table.keyboardDismissMode = .onDrag
        table.refreshControl = refresh
        host.searchBar.delegate = self
        refresh.addTarget(self, action: #selector(didPullToRefresh), for: .valueChanged)
        return host
    }()

    @objc public override func createView() -> UIView {
        host
    }

    // MARK: - Rust から

    func setRows(_ json: String) {
        rows = Self.parse(json)
        host.table.reloadData()
        updateBackground()
        updateCount()
    }

    func setHeader(_ text: String) {
        header = text
        updateCount()
    }

    private func updateCount() {
        let show = !rows.isEmpty && !header.isEmpty
        countLabel.text = header
        if show, host.table.tableHeaderView !== countHeader {
            countHeader.frame.size.width = host.table.bounds.width
            host.table.tableHeaderView = countHeader
        } else if !show, host.table.tableHeaderView != nil {
            host.table.tableHeaderView = nil
        }
    }

    func setMessage(_ text: String) {
        messageLabel.text = text
        updateBackground()
    }

    func setPlaceholder(_ text: String) {
        searchBar.placeholder = text
    }

    func setRefreshing(_ on: Bool) {
        if !on, refresh.isRefreshing { refresh.endRefreshing() }
    }

    func setBottomInset(_ inset: CGFloat) {
        host.table.contentInset.bottom = inset
        host.table.verticalScrollIndicatorInsets.bottom = inset
    }

    private func updateBackground() {
        let empty = rows.isEmpty && !(messageLabel.text ?? "").isEmpty
        host.table.backgroundView = empty ? messageLabel : nil
    }

    // MARK: - 表示

    public func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        rows.count
    }

    public func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let row = rows[indexPath.row]
        let cell = tableView.dequeueReusableCell(withIdentifier: "row")
            ?? UITableViewCell(style: .default, reuseIdentifier: "row")
        var content = UIListContentConfiguration.subtitleCell()
        content.text = row.title
        content.textProperties.numberOfLines = 1
        content.secondaryText = row.subtitle.isEmpty ? nil : row.subtitle
        content.secondaryTextProperties.color = .secondaryLabel
        content.secondaryTextProperties.numberOfLines = 1
        content.textToSecondaryTextVerticalPadding = 3
        content.image = row.avatar ? Self.avatar(for: row.title) : nil
        cell.contentConfiguration = content
        cell.accessoryView = Self.trailing(row.trailing)
        return cell
    }

    // MARK: - 操作

    public func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        WhiskerCustomEvent.dispatch(from: self, name: "select", params: ["key": rows[indexPath.row].key])
    }

    public func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "search", params: ["value": searchText])
    }

    public func searchBarSearchButtonClicked(_ searchBar: UISearchBar) {
        searchBar.resignFirstResponder()
    }

    @objc private func didPullToRefresh() {
        WhiskerCustomEvent.dispatch(from: self, name: "refresh", params: [:])
    }

    // MARK: - 部品

    /// 右端：状態の灰色の文字と「›」。
    private static func trailing(_ text: String) -> UIView {
        let chevron = UIImageView(
            image: UIImage(systemName: "chevron.right",
                           withConfiguration: UIImage.SymbolConfiguration(pointSize: 13, weight: .semibold)))
        chevron.tintColor = .tertiaryLabel
        let label = UILabel()
        label.text = text
        label.textColor = .secondaryLabel
        label.font = .preferredFont(forTextStyle: .subheadline)
        let stack = UIStackView(arrangedSubviews: text.isEmpty ? [chevron] : [label, chevron])
        stack.spacing = 8
        stack.alignment = .center
        stack.frame.size = stack.systemLayoutSizeFitting(UIView.layoutFittingCompressedSize)
        return stack
    }

    /// 名前の頭文字を灰色の丸に白で描く（連絡先アプリ風）。
    private static func avatar(for name: String) -> UIImage {
        let initial = name.first(where: { $0.isLetter || $0.isNumber }).map { String($0).uppercased() } ?? "?"
        let size = CGSize(width: 40, height: 40)
        return UIGraphicsImageRenderer(size: size).image { _ in
            UIColor.systemGray3.setFill()
            UIBezierPath(ovalIn: CGRect(origin: .zero, size: size)).fill()
            let attrs: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: 17, weight: .semibold),
                .foregroundColor: UIColor.white,
            ]
            let text = initial as NSString
            let t = text.size(withAttributes: attrs)
            text.draw(at: CGPoint(x: (size.width - t.width) / 2, y: (size.height - t.height) / 2),
                      withAttributes: attrs)
        }
    }

    private static func parse(_ json: String) -> [RecordRow] {
        guard let data = json.data(using: .utf8),
              let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return [] }
        return list.map { row in
            RecordRow(
                key: row["key"] as? String ?? "",
                title: row["title"] as? String ?? "",
                subtitle: row["subtitle"] as? String ?? "",
                trailing: row["trailing"] as? String ?? "",
                avatar: row["avatar"] as? Bool ?? false
            )
        }
    }
}
#endif
