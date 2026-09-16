//! Offline scrolling benchmark using the production view and real renderers.
//! GPU timings wait for completion; CPU timings use iced's damage tracking.
//! Neither includes screenshot readback, window presentation, or vsync.
use comet_lib::ui::{fixtures, Comet, Dialog, Message};
use iced::advanced::renderer::Headless as _;
use iced::{
    advanced::{graphics, renderer, Clipboard},
    mouse, window, Event, Point, Rectangle, Renderer, Size,
};
use iced_test::{
    futures::futures::executor::block_on,
    runtime::{user_interface::Cache, UserInterface},
};
use iced_wgpu::wgpu;
use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const SCALE: f32 = 2.0;
const WARMUP: usize = 8;
const SAMPLES: usize = 30;

struct NoClipboard;
impl Clipboard for NoClipboard {
    fn read(&self, _: iced::advanced::clipboard::Kind) -> Option<String> {
        None
    }
    fn write(&mut self, _: iced::advanced::clipboard::Kind, _: String) {}
}

struct Gpu {
    device: wgpu::Device,
    engine: iced_wgpu::Engine,
    format: wgpu::TextureFormat,
}
impl Gpu {
    async fn new() -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: wgpu::InstanceFlags::empty(),
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .expect("A hardware GPU is required for this benchmark");
        let info = adapter.get_info();
        assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
        println!(
            "GPU: {} ({:?}, {:?})",
            info.name, info.backend, info.device_type
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Comet rendering benchmark"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits {
                    max_bind_groups: 2,
                    max_non_sampler_bindings: 2048,
                    ..Default::default()
                },
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            })
            .await
            .expect("Create GPU device");
        let format = if graphics::color::GAMMA_CORRECTION {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue,
            format,
            None, // Match the desktop app's default antialiasing setting.
            graphics::Shell::headless(),
        );
        Self {
            device,
            engine,
            format,
        }
    }
}

enum Target {
    Cpu {
        pixmap: tiny_skia::Pixmap,
        mask: tiny_skia::Mask,
        previous: Option<Vec<iced_tiny_skia::Layer>>,
    },
    Gpu {
        device: wgpu::Device,
        texture: wgpu::Texture,
        format: wgpu::TextureFormat,
    },
}
impl Target {
    fn new(gpu: Option<&Gpu>) -> (Renderer, Self) {
        match gpu {
            Some(gpu) => {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Comet benchmark target"),
                    size: wgpu::Extent3d {
                        width: WIDTH * 2,
                        height: HEIGHT * 2,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: gpu.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                (
                    Renderer::Primary(iced_wgpu::Renderer::new(
                        gpu.engine.clone(),
                        iced_m3::fonts::REGULAR,
                        16.into(),
                    )),
                    Self::Gpu {
                        device: gpu.device.clone(),
                        texture,
                        format: gpu.format,
                    },
                )
            }
            None => (
                Renderer::Secondary(iced_tiny_skia::Renderer::new(
                    iced_m3::fonts::REGULAR,
                    16.into(),
                )),
                Self::Cpu {
                    pixmap: tiny_skia::Pixmap::new(WIDTH * 2, HEIGHT * 2).unwrap(),
                    mask: tiny_skia::Mask::new(WIDTH * 2, HEIGHT * 2).unwrap(),
                    previous: None,
                },
            ),
        }
    }

    fn render(
        &mut self,
        renderer: &mut Renderer,
        viewport: &graphics::Viewport,
        background: iced::Color,
    ) {
        match (self, renderer) {
            (
                Self::Cpu {
                    pixmap,
                    mask,
                    previous,
                },
                Renderer::Secondary(renderer),
            ) => {
                let bounds = Rectangle::with_size(viewport.logical_size());
                let damage = previous.as_ref().map_or_else(
                    || vec![bounds],
                    |previous| {
                        graphics::damage::diff(
                            previous,
                            renderer.layers(),
                            |layer| vec![layer.bounds],
                            iced_tiny_skia::Layer::damage,
                        )
                    },
                );
                *previous = Some(renderer.layers().to_vec());
                let damage = graphics::damage::group(damage, bounds);
                if !damage.is_empty() {
                    renderer.draw(&mut pixmap.as_mut(), mask, viewport, &damage, background);
                }
            }
            (
                Self::Gpu {
                    device,
                    texture,
                    format,
                },
                Renderer::Primary(renderer),
            ) => {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let submission = renderer.present(Some(background), *format, &view, viewport);
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(30)),
                    })
                    .expect("GPU render must complete");
            }
            _ => unreachable!("The renderer and target must use the same backend"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Scene {
    Form,
    DialogForm,
    Settings,
}
fn view(app: &Comet, scene: Scene) -> iced_m3::Element<'_, Message> {
    if matches!(scene, Scene::Settings) {
        return app.view();
    }
    let content = iced::widget::Column::with_children((0..12).map(|_| {
        iced_m3::text_field("Setting", "sample value")
            .on_input(Message::AccountName)
            .into()
    }))
    .spacing(16);
    if matches!(scene, Scene::Form) {
        iced::widget::container(iced::widget::scrollable(content).height(600).width(640))
            .center(iced::Length::Fill)
            .into()
    } else {
        iced_m3::dialog::host(
            iced::widget::Space::new()
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            iced_m3::dialog::dialog(content).width(640.0),
            true,
        )
    }
}

fn run(scene: Scene, gpu: Option<&Gpu>, output: Option<&Path>) {
    let mut app = fixtures::notebook();
    app.dialog.show(Dialog::Settings);
    let theme = app.theme();
    let (mut renderer, mut target) = Target::new(gpu);
    let viewport = graphics::Viewport::with_physical_size(Size::new(WIDTH * 2, HEIGHT * 2), SCALE);
    let mut ui = UserInterface::build(
        view(&app, scene),
        viewport.logical_size(),
        Cache::default(),
        &mut renderer,
    );
    let mut clipboard = NoClipboard;
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut work = Vec::with_capacity(SAMPLES);
    let cursor = mouse::Cursor::Available(Point::new(640.0, 300.0));
    for frame in 0..WARMUP + SAMPLES {
        let start = Instant::now();
        let mut messages = Vec::new();
        ui.update(
            &[
                Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines {
                        x: 0.0,
                        y: if frame % 6 < 3 { -1.0 } else { 1.0 },
                    },
                }),
                Event::Window(window::Event::RedrawRequested(
                    Instant::now() + Duration::from_secs(5),
                )),
            ],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert!(messages.is_empty());
        ui.draw(
            &mut renderer,
            &theme,
            &renderer::Style {
                text_color: theme.colors.on_surface,
            },
            cursor,
        );
        let ui_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        target.render(&mut renderer, &viewport, theme.colors.surface);
        let render_ms = start.elapsed().as_secs_f64() * 1000.0;
        if frame >= WARMUP {
            samples.push(render_ms);
            work.push(ui_ms);
        }
    }
    samples.sort_by(f64::total_cmp);
    work.sort_by(f64::total_cmp);
    println!(
        "{:9} {scene:10?} repaint median={:.3} ms p95={:.3} ms; UI median={:.3} ms",
        if gpu.is_some() { "wgpu" } else { "tiny-skia" },
        samples[SAMPLES / 2],
        samples[SAMPLES * 95 / 100],
        work[SAMPLES / 2]
    );
    // Optional visual inspection happens after all timing samples, so readback
    // and PNG encoding cannot inflate GPU or CPU repaint measurements.
    if let Some(output) = output {
        std::fs::create_dir_all(output).unwrap();
        let backend = if gpu.is_some() { "wgpu" } else { "tiny-skia" };
        let path = output.join(format!("{backend}-{scene:?}.png"));
        let rgba = renderer.screenshot(viewport.physical_size(), SCALE, theme.colors.surface);
        let mut png = png::Encoder::new(
            BufWriter::new(File::create(path).unwrap()),
            WIDTH * 2,
            HEIGHT * 2,
        );
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header().unwrap().write_image_data(&rgba).unwrap();
    }
}
fn main() {
    let backend = std::env::args().nth(1).unwrap_or_else(|| "both".into());
    assert!(
        ["both", "wgpu", "tiny-skia"].contains(&backend.as_str()),
        "Usage: rendering_benchmark [both|wgpu|tiny-skia] [screenshot-directory]"
    );
    let output = std::env::args().nth(2).map(PathBuf::from);
    println!("{WIDTH}x{HEIGHT} logical pixels, {SCALE}x scale; {WARMUP} warmup + {SAMPLES} measured frames");
    iced::advanced::graphics::text::font_system()
        .write()
        .unwrap()
        .load_font(std::borrow::Cow::Borrowed(iced_m3::fonts::ROBOTO));
    let gpu = (backend != "tiny-skia").then(|| block_on(Gpu::new()));
    for scene in [Scene::Form, Scene::DialogForm, Scene::Settings] {
        if backend != "wgpu" {
            run(scene, None, output.as_deref());
        }
        if let Some(gpu) = &gpu {
            run(scene, Some(gpu), output.as_deref());
        }
    }
}
