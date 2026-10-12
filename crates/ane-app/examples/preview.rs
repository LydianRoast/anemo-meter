//! Render the actual disconnected application for layout review; no synthetic readings.
use eframe::{egui, App};
use std::path::PathBuf;

struct Preview {
    app: ane_app::app::Console,
    frames: usize,
    destination: PathBuf,
}
impl App for Preview {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        self.frames += 1;
        if self.frames == 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        for event in ctx.input(|input| input.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                let buffer =
                    image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, bytes)
                        .expect("RGBA dimensions");
                if let Some(folder) = self.destination.parent() {
                    std::fs::create_dir_all(folder).unwrap();
                }
                buffer
                    .save(&self.destination)
                    .expect("Save actual application screenshot");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.frames > 100 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let destination = PathBuf::from(args.get(1).expect("PNG output path required"));
    let phone = args.iter().any(|arg| arg == "--phone");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(if phone {
            [390.0, 820.0]
        } else {
            [1060.0, 820.0]
        }),
        ..Default::default()
    };
    eframe::run_native(
        "ANE-200W layout review",
        options,
        Box::new(move |cc| {
            Ok(Box::new(Preview {
                app: ane_app::app::Console::new(cc, None),
                frames: 0,
                destination,
            }))
        }),
    )
}
