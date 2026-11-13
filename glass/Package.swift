// swift-tools-version:5.9
//
// field-glass の iOS 側。Apple のガラス効果（iOS 26 以降の UIGlassEffect）を
// Whisker の部品として登録する。作りは whisker-input と同じ。

import PackageDescription

let package = Package(
    name: "field-glass",
    platforms: [.iOS(.v15)],
    products: [
        .library(name: "FieldGlass", targets: ["FieldGlass"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.21"),
    ],
    targets: [
        .target(
            name: "FieldGlass",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/FieldGlass",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
