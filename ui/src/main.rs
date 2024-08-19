#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example
#![allow(unreachable_code)]
#![feature(decl_macro)]

use std::{borrow::Borrow, path::PathBuf, time::Duration};

use anyhow::Result as Rest;
use clap::{Command, Parser};
use eframe::egui;

use egui_plot::{self, Legend};
use futures::StreamExt;
use futures::{channel::mpsc::*, SinkExt};
use nsproxy_common::forever;
use nsproxy_common::rpc::Data;
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};

#[derive(Parser)]
#[command()]
struct Cli {
    /// path to socket
    #[arg(long, short)]
    sock: Option<PathBuf>,
}

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
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        ..Default::default()
    };

    let cmd = Cli::parse();

    let (sx, rx) = unbounded();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            type Codec = Bincode<FromClient, FromServer>;
            use nsproxy_common::rpc::*;
            use tarpc::serde_transport::unix;
            use tarpc::tokio_serde::formats::*;
            if let Some(sock) = cmd.sock {
                let mut p = unix::connect(sock, Codec::default).await?;
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
            } else {
                todo!()
            }

            forever!().await;
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
                egui_plot::Plot::new("plot")
                    .legend(Legend::default())
                    .show(ui, |pu| {
                        use egui_plot::*;
                        pu.line(Line::new(PlotPoints::Owned(
                            self.ns
                                .loop_time
                                .iter()
                                .enumerate()
                                .map(|(x, y)| PlotPoint::new(x as f64, y.as_millis() as f64))
                                .collect(),
                        )));
                    })
            })
        });
        ctx.request_repaint_after(Duration::from_millis(5));
    }
}
