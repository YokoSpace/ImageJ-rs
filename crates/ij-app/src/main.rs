use ij_app::ImageJApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([680.0, 200.0])
            .with_min_inner_size([600.0, 95.0])
            .with_title("ImageJ 1.54u")
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "ImageJ",
        native_options,
        Box::new(move |cc| {
            ij_app::macos_bridge::install_macos_termination_bridge(Some(&cc.egui_ctx));
            let mut app = ImageJApp::new();
            app.try_detect_plugin_bridge();
            for arg in &args {
                if !arg.starts_with('-') {
                    let path = std::path::PathBuf::from(arg);
                    if path.exists() {
                        let _ = app.open_tiff_file(path);
                    }
                }
            }
            Ok(Box::new(app))
        }),
    )
}
