#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example
#![allow(unreachable_code)]
#![feature(decl_macro)]

use std::{borrow::Borrow, time::Duration};

use anyhow::Result as Rest;
use eframe::egui;
use egui_plotter::{
    plotters::{
        chart::ChartBuilder,
        prelude::IntoDrawingArea,
        series::LineSeries,
        style::{full_palette::WHITE, Color},
    },
    EguiBackend,
};
use futures::StreamExt;
use futures::{channel::mpsc::*, SinkExt};
use nsproxy_common::rpc::Data;
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};

macro aok($t:ty) {
    Rest::<$t, anyhow::Error>::Ok(())
}

struct NSState {
    loop_time: HeapRb<Duration>,
}

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    let (sx, rx) = unbounded();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            use tarpc::serde_transport::unix;
            let p = unix::TempPathBuf::new(TMP_RPC_PATH);
            use nsproxy_common::rpc::*;
            use tarpc::tokio_serde::formats::*;
            let mut s = unix::listen(p, Bincode::<FromClient, FromServer>::default).await?;
            loop {
                if let Some(p) = s.next().await {
                    let mut p = p?;
                    let mut sx = sx.clone();
                    tokio::spawn(async move {
                        loop {
                            let msg = p.next().await;
                            if let Some(msg) = msg {
                                let msg = msg?;
                                match msg {
                                    FromClient::Data(d) => {
                                        sx.send(d).await?;
                                    }
                                    _ => {
                                        // todo
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        aok!(())
                    });
                }
            }
            aok!(())
        })
        .unwrap();
    });
    eframe::run_native(
        "nsproxy",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<MyApp>::new(MyApp {
                ns: NSState {
                    loop_time: HeapRb::new(128),
                },
                rx,
            }))
        }),
    )
}

struct MyApp {
    ns: NSState,
    rx: UnboundedReceiver<Data>,
}

impl NSState {
    fn apply(&mut self, d: Data) {
        match d {
            Data::LoopTime(d) => {
                self.loop_time.push_overwrite(d);
            }
        };
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        loop {
            match self.rx.try_next() {
                Ok(Some(msg)) => self.ns.apply(msg),
                _ => break,
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("performance");
            egui::Frame::default().show(ui, |ui| {
                let f = || {
                    let r = EguiBackend::new(ui).into_drawing_area();
                    let mut cb = ChartBuilder::on(&r);
                    let mut cx = cb.build_cartesian_2d(0..128, 0..59).unwrap();
                    cx.configure_mesh()
                        .light_line_style(WHITE.mix(0.005))
                        .bold_line_style(WHITE.mix(0.01))
                        .draw()?;
                    cx.draw_series(LineSeries::new(
                        self.ns
                            .loop_time
                            .iter()
                            .enumerate()
                            .map(|(x, y)| (x as i32, y.as_millis() as i32)),
                        WHITE.mix(0.03),
                    ))?;
                    aok!(())
                };
                f().unwrap();
            })
        });
    }
}
