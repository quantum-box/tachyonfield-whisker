fn main() {
    whisker_cng::run(|app| {
        app.name("TACHYON Field")
            .bundle_id("app.txcloud.tachyonfield.whisker")
            .background("#FFFFFF")
            .version("0.1.0")
            .build_number(1);

        app.android(|a| {
            a.package("app.txcloud.tachyonfield.whisker")
                .application_id("app.txcloud.tachyonfield.whisker")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(34);
        });

        app.ios(|i| {
            i.bundle_id("app.txcloud.tachyonfield.whisker")
                .scheme("TACHYON Field")
                .deployment_target("15.0");
        });
    });
}
