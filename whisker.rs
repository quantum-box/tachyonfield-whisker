fn main() {
    // CI sets BUILD_NUMBER (the workflow run number) so every TestFlight
    // upload gets a new build number. Local builds use 1.
    let build_number = std::env::var("BUILD_NUMBER")
        .ok()
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(1);
    whisker_cng::run(move |app| {
        app.name("TACHYON Field")
            .bundle_id("com.quantumbox.tachyonfield")
            .background("#FFFFFF")
            .version("0.2.0")
            .build_number(build_number);

        app.android(|a| {
            a.package("app.txcloud.tachyonfield.whisker")
                .application_id("app.txcloud.tachyonfield.whisker")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(34);
        });

        app.ios(|i| {
            i.bundle_id("com.quantumbox.tachyonfield")
                .scheme("TACHYON Field")
                .deployment_target("15.0");
        });
    });
}
