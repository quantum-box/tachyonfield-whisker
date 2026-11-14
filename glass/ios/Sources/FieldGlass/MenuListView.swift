// iOS 標準の一覧（UITableView の insetGrouped）。設定アプリと同じ見た目・押し心地。
//
// Rust からは `sections` を JSON の文字で受け取る：
//   [{ "title": "販売管理",
//      "rows": [{ "key": "案件", "title": "案件", "subtitle": "…", "symbol": "briefcase.fill",
//                 "color": "#007AFF", "accessory": "chevron", "destructive": false }] }]
// - symbol … SF Symbols の名前。色付きの角丸の四角に白で描く（空ならアイコンなし）
// - accessory … 右端の印。"chevron"（›、既定）/ "check"（✓）/ "expand"（⌄）/ "collapse"（⌃）/ "none"
// - destructive … true なら赤字で印なし（サインアウトなど）
// 行が押されたら "select" を { key } 付きで Rust に知らせる。
//
// 中身の描き方（`MenuTableModel`）は、下から出るシート（`SheetView.swift`）でも使う。

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// 一覧を入れる箱。Whisker の入れ物に直接置くと UIKit の部品が中身を並べ直さないので、
/// 大きさが決まるたびに一覧の大きさを付け直す。
private final class MenuListHost: UIView {
    let table = UITableView(frame: .zero, style: .insetGrouped)

    override init(frame: CGRect) {
        super.init(frame: frame)
        addSubview(table)
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        table.frame = bounds
    }
}

struct MenuRow {
    let key: String
    let title: String
    let subtitle: String
    let symbol: String
    let color: UIColor
    let accessory: String
    let destructive: Bool
}

struct MenuSection {
    let title: String
    let rows: [MenuRow]
}

/// 見出し付きの行を UITableView に描き、押された行の key を `onSelect` で返す。
final class MenuTableModel: NSObject, UITableViewDataSource, UITableViewDelegate {
    var sections: [MenuSection] = []
    var onSelect: ((String) -> Void)?
    private let swipeGuard = SwipeGuard()

    func attach(to table: UITableView) {
        table.dataSource = self
        table.delegate = self
        swipeGuard.attach(to: table)
        table.register(UITableViewCell.self, forCellReuseIdentifier: "row")
    }

    func setSections(_ json: String) {
        sections = Self.parse(json)
    }

    func numberOfSections(in tableView: UITableView) -> Int {
        sections.count
    }

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        sections[section].rows.count
    }

    func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        let title = sections[section].title
        return title.isEmpty ? nil : title
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let row = sections[indexPath.section].rows[indexPath.row]
        let cell = tableView.dequeueReusableCell(withIdentifier: "row", for: indexPath)
        var content = row.subtitle.isEmpty
            ? cell.defaultContentConfiguration() : UIListContentConfiguration.subtitleCell()
        content.text = row.title
        content.secondaryText = row.subtitle.isEmpty ? nil : row.subtitle
        content.secondaryTextProperties.color = .secondaryLabel
        content.secondaryTextProperties.numberOfLines = 1
        cell.accessoryView = nil
        cell.accessoryType = .none
        if row.destructive {
            content.textProperties.color = .systemRed
            content.image = nil
        } else {
            content.textProperties.color = .label
            content.image = row.symbol.isEmpty ? nil : Self.icon(symbol: row.symbol, color: row.color)
            switch row.accessory {
            case "check": cell.accessoryType = .checkmark
            case "expand", "collapse":
                let name = row.accessory == "expand" ? "chevron.down" : "chevron.up"
                let image = UIImageView(image: UIImage(
                    systemName: name,
                    withConfiguration: UIImage.SymbolConfiguration(pointSize: 13, weight: .semibold)))
                image.tintColor = .tertiaryLabel
                cell.accessoryView = image
            case "none": break
            default: cell.accessoryType = .disclosureIndicator
            }
        }
        cell.contentConfiguration = content
        return cell
    }

    func tableView(_ tableView: UITableView, willSelectRowAt indexPath: IndexPath) -> IndexPath? {
        swipeGuard.allowsSelection ? indexPath : nil
    }

    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        onSelect?(sections[indexPath.section].rows[indexPath.row].key)
    }

    /// 設定アプリのアイコン：色付きの角丸の四角に、白い SF Symbol。
    private static func icon(symbol: String, color: UIColor) -> UIImage {
        let size = CGSize(width: 29, height: 29)
        return UIGraphicsImageRenderer(size: size).image { _ in
            let rect = CGRect(origin: .zero, size: size)
            color.setFill()
            UIBezierPath(roundedRect: rect, cornerRadius: 7).fill()
            let config = UIImage.SymbolConfiguration(pointSize: 15, weight: .semibold)
            if let glyph = UIImage(systemName: symbol, withConfiguration: config)?
                .withTintColor(.white, renderingMode: .alwaysOriginal)
            {
                let g = glyph.size
                glyph.draw(in: CGRect(x: (size.width - g.width) / 2, y: (size.height - g.height) / 2,
                                      width: g.width, height: g.height))
            }
        }
    }

    private static func parse(_ json: String) -> [MenuSection] {
        guard let data = json.data(using: .utf8),
              let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return [] }
        return list.map { section in
            let rows = (section["rows"] as? [[String: Any]] ?? []).map { row in
                MenuRow(
                    key: row["key"] as? String ?? "",
                    title: row["title"] as? String ?? "",
                    subtitle: row["subtitle"] as? String ?? "",
                    symbol: row["symbol"] as? String ?? "",
                    color: color(hex: row["color"] as? String ?? ""),
                    accessory: row["accessory"] as? String ?? "chevron",
                    destructive: row["destructive"] as? Bool ?? false
                )
            }
            return MenuSection(title: section["title"] as? String ?? "", rows: rows)
        }
    }

    private static func color(hex: String) -> UIColor {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard let value = UInt32(digits, radix: 16) else { return .systemGray }
        return UIColor(
            red: CGFloat((value >> 16) & 0xFF) / 255,
            green: CGFloat((value >> 8) & 0xFF) / 255,
            blue: CGFloat(value & 0xFF) / 255,
            alpha: 1
        )
    }
}

@objc(FieldMenuListView)
public final class FieldMenuListView: WhiskerUI<UIView> {
    private let model = MenuTableModel()

    private lazy var host: MenuListHost = {
        let host = MenuListHost()
        model.attach(to: host.table)
        model.onSelect = { [weak self] key in
            guard let self else { return }
            WhiskerCustomEvent.dispatch(from: self, name: "select", params: ["key": key])
        }
        return host
    }()

    @objc public override func createView() -> UIView {
        host
    }

    func setSections(_ json: String) {
        model.setSections(json)
        host.table.reloadData()
    }
}
#endif
