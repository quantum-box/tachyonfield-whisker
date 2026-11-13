# TACHYON Field (Whisker)

The TACHYON Field mobile app, built in Rust with [Whisker](https://github.com/whiskerrs/whisker).
It talks to the TACHYON Field API and uses native iOS parts (UITabBar, UITableView,
Liquid Glass, sheets) through the `glass/` (`field-glass`) Whisker module.

- Sign in with the same Cognito client as the Field web app
- Switch companies from the sidebar (searchable sheet)
- Areas in the sidebar decide the bottom tabs
- Lists with search and pull to refresh
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
