# TACHYON Field (Whisker)

The TACHYON Field mobile app, built in Rust with [Whisker](https://github.com/whiskerrs/whisker).
It talks to the TACHYON Field API and uses native iOS parts (UITabBar, UITableView,
Liquid Glass, sheets) through the `glass/` (`field-glass`) Whisker module.

- Sign in with the same Cognito client as the Field web app
- Switch companies from the sidebar (searchable sheet)
- Areas in the sidebar decide the bottom tabs
- Lists with search and pull to refresh
- Searchable native management menus, typed forms, reference pickers and line items
- Quotation and order entry, sales-contract lifecycle actions, invoice payment links,
  PDF email delivery and cancellation with reversal review
- Product stock receipts, issues and adjustments, location transfers, and stocktaking
  from counted quantities through finalization
- Daily-report submission/review, store opening checks, attendance CSV preview/import,
  accounting workflows, reports and analysis
- Reservation calendars, start-slot picking, customer and plan selection, rescheduling,
  payment links, cancellation/refund recovery, waitlists, handoff notes and notifications
- Field Agent chat, streamed over SSE

Licensed under the [MIT License](LICENSE).

## Develop

```sh
# On an iOS Simulator (macOS only).
whisker run ios

# On an Android device or emulator.
whisker run android
```

Run `whisker doctor` first to verify your toolchain is set up for each
target.

## Management coverage

The native management screens are compared with the Field web app's 113 top-level
menu entries. The catalog has 161 screen definitions covering 110 of those menus;
existing dedicated screens provide the remaining three. The
[coverage matrix](docs/native-management-parity.md) lists the implemented reads,
writes and remaining differences for every menu. Some menus provide read access
while specialized operations, exports or external integrations remain unfinished.

The [sales and workforce notes](docs/native-management-sales.md),
[inventory and accounting notes](docs/native-management-operations.md),
[invoice delivery and cancellation notes](docs/native-management-invoices.md) and
[analysis notes](docs/native-management-analytics.md) explain the supported workflows
and their limits. Product stock and location stock use separate workflows. Invoice
PDF delivery shows when sending can activate a draft sales contract, and invoice
cancellation shows the reversal plan before execution.

The [reservation workflow notes](docs/native-reservations.md) cover booking and
payment actions, waitlist promotion, scheduling, legacy settings and iOS CSV/link
exports. The app distinguishes unknown write outcomes from preflight rejections;
reservation cancellation can require history review even after an HTTP 400.

Existing sales and customer tabs open the same native management components as the
management menu. The app keeps the dedicated strategy, cash-book, register-closing,
receipt-OCR and Agent screens. Document picking and previews currently use the iOS
`field-glass` components; their platform limits are called out in the UI.

`src/management/schema.rs` describes verified API contracts. The domain catalogs
provide explicit read fields, typed inputs, allowed state transitions and request
builders. `model.rs`, `form.rs` and `screen.rs` share exact currency conversion,
pagination, reference selection, optimistic versions, confirmations and duplicate
submission handling. Inventory and invoice workflow adapters add verified commands
to their existing catalog entries; coverage is counted after those adapters run.
Specialized analysis and attendance-import screens handle
workflows that need more than one request. The server remains authoritative for
authorization and business-state validation.

Run the contract, data-conversion and native component regressions with:

```sh
cargo test --locked
```

Pull-request CI runs these tests and an unsigned iOS Simulator build. Headless tests
do not replace checking the app on an actual device with an authorized test tenant.

## Edit

The UI lives in [`src/lib.rs`](src/lib.rs). Save any change and
`whisker run` hot-patches the running app in under a second, without a
restart. Only the components you edited are rebuilt, so state owned by
the rest of the tree is kept. Edits outside components (a helper
function, a type) or to `Root` rebuild the whole UI.

App-level metadata (bundle id, app name, Android / iOS deployment
settings) lives in [`whisker.rs`](whisker.rs). `whisker run` and
`whisker build` generate the native projects under `gen/` from it, so
there is nothing else to run. Edits there need a `whisker run` restart
since they reshape the generated project. Add plugin crates with
`cargo add`, then configure them in `whisker.rs`.

### Opening the native project in Xcode or Android Studio

To generate `gen/` without building or launching the app, for example to
open `gen/ios` in Xcode before the first `whisker run`:

```sh
cargo run --bin whisker-config --features whisker-config -- ios
```

Pass `android`, `desktop`, or `web` instead, or omit the platform to
generate all four.

## Build for release

Build the generated projects with Xcode or Gradle, or use `whisker build`:

```sh
# Android release APK
( cd gen/android && ./gradlew :app:assembleRelease )

# iOS Simulator .app (Release configuration)
xcodebuild -project gen/ios/<Scheme>.xcodeproj \
  -scheme <Scheme> -configuration Release \
  -destination 'generic/platform=iOS Simulator' build
```

The `gen/` tree is refreshed automatically on every `whisker run`;
delete it whenever you want a clean re-generate.
