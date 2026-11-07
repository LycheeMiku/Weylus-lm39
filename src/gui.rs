use std::cmp::min;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use tracing::{error, info, warn};

use fltk::{
    app::{awake_callback, App},
    button::{Button, CheckButton},
    enums::{Color, Font, FrameType, LabelType},
    frame::Frame,
    image::PngImage,
    input::{Input, IntInput},
    menu::Choice,
    output::Output,
    prelude::*,
    text::{StyleTableEntry, TextBuffer, TextDisplay},
    window::Window,
};

use crate::config::{write_config, Config, ThemeType};
use crate::protocol::{CustomInputAreas, Rect};
use crate::web::Web2UiMessage;

fn get_lan_ip(bind_addr: IpAddr) -> IpAddr {
    if !bind_addr.is_unspecified() {
        return bind_addr;
    }
    // Probe local route via UDP socket
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    return ip;
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for iface in pnet_datalink::interfaces()
            .iter()
            .filter(|iface| iface.is_up() && !iface.is_loopback())
        {
            for ipnetw in &iface.ips {
                if ipnetw.is_ipv4() {
                    return ipnetw.ip();
                }
            }
        }
    }
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

fn render_qr_to_frame(qr_frame: &mut Frame, url: &str, size: i32) {
    if let Ok(code) = qrcode::QrCode::new(url) {
        let img_buf = code.render::<image::Luma<u8>>().build();
        let image = image::DynamicImage::ImageLuma8(img_buf);
        let dims = size.max(100) as u32;
        let image = image.resize_exact(dims, dims, image::imageops::FilterType::Nearest);
        let mut buf = vec![];
        let mut cursor = Cursor::new(&mut buf);
        if image.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
            if let Ok(png) = PngImage::from_data(&buf) {
                qr_frame.set_image(Some(png));
                qr_frame.redraw();
            }
        }
    }
}

fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        if let (Ok(u), Ok(op)) = (CString::new(url), CString::new("open")) {
            unsafe {
                winapi::um::shellapi::ShellExecuteA(
                    std::ptr::null_mut(),
                    op.as_ptr(),
                    u.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    winapi::um::winuser::SW_SHOWNORMAL,
                );
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn confirm_dialog(parent: &Window, title: &str, message: &str) -> bool {
    let w = 420;
    let h = 140;
    let px = parent.x() + (parent.w() - w).max(0) / 2;
    let py = parent.y() + (parent.h() - h).max(0) / 2;

    let mut win = Window::default()
        .with_pos(px, py)
        .with_size(w, h)
        .with_label(title);
    win.make_modal(true);
    win.set_xclass("weylus");

    let mut frame = Frame::default()
        .with_pos(20, 20)
        .with_size(w - 40, 50)
        .with_label(message);
    frame.set_label_size(13);

    let confirmed = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut btn_cancel = Button::default()
        .with_pos(w - 180, h - 45)
        .with_size(75, 28)
        .with_label("Cancel");

    let mut btn_ok = Button::default()
        .with_pos(w - 95, h - 45)
        .with_size(75, 28)
        .with_label("OK");

    win.end();
    win.show();

    let mut win_close = win.clone();
    btn_cancel.set_callback(move |_| {
        win_close.hide();
    });

    let mut win_close2 = win.clone();
    let confirmed_clone = confirmed.clone();
    btn_ok.set_callback(move |_| {
        confirmed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        win_close2.hide();
    });

    let confirmed_clone2 = confirmed.clone();
    let mut win_close3 = win.clone();
    win.handle(move |_w, ev| match ev {
        fltk::enums::Event::KeyDown => {
            if fltk::app::event_key() == fltk::enums::Key::Enter {
                confirmed_clone2.store(true, std::sync::atomic::Ordering::SeqCst);
                win_close3.hide();
                true
            } else if fltk::app::event_key() == fltk::enums::Key::Escape {
                win_close3.hide();
                true
            } else {
                false
            }
        }
        _ => false,
    });

    while win.shown() {
        fltk::app::wait();
    }

    confirmed.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn run(config: &Config, log_receiver: mpsc::Receiver<String>) {
    let width = 170;
    let height = 24;

    let app = App::default().with_scheme(fltk::app::AppScheme::Gtk);
    config.gui_theme.map(|th| th.apply());

    let initial_height = if config.enable_log { 536 } else { 304 };
    let mut wind = Window::default()
        .with_size(660, initial_height)
        .center_screen()
        .with_label(&format!("Weylus - Graphics Tablet v{}", env!("CARGO_PKG_VERSION")));
    wind.set_xclass("weylus");

    let is_server_running = Arc::new(AtomicBool::new(false));
    {
        let is_server_running = is_server_running.clone();
        wind.set_callback(move |win| {
            if is_server_running.load(std::sync::atomic::Ordering::SeqCst) {
                if confirm_dialog(win, "Exit Confirmation", "Server is currently running. Are you sure you want to exit?") {
                    app.quit();
                }
            } else {
                app.quit();
            }
        });
    }

    wind.size_range(660, initial_height, 660, if config.enable_log { 0 } else { 304 });

    // Left Column: Server settings & control
    let mut input_access_code = Input::default()
        .with_pos(110, 16)
        .with_size(width, height)
        .with_label("Access Code");
    if let Some(code) = config.access_code.as_ref() {
        input_access_code.set_value(code);
    }

    let mut input_bind_addr = Input::default()
        .with_pos(110, 46)
        .with_size(width, height)
        .with_label("Bind Addr");
    input_bind_addr.set_value(&config.bind_address.to_string());

    let mut input_port = IntInput::default()
        .with_pos(110, 76)
        .with_size(width, height)
        .with_label("Port");
    input_port.set_value(&config.web_port.to_string());

    let mut choice_theme = Choice::default()
        .with_pos(110, 106)
        .with_size(width, height)
        .with_label("Theme");
    for theme in ThemeType::themes() {
        choice_theme.add_choice(&theme.name());
    }
    choice_theme.set_value(config.gui_theme.unwrap_or_default().to_index());

    let check_auto_start = CheckButton::default()
        .with_pos(110, 136)
        .with_size(90, 22)
        .with_label("Auto Start");
    check_auto_start.set_checked(config.auto_start);

    let mut check_enable_log = CheckButton::default()
        .with_pos(205, 136)
        .with_size(85, 22)
        .with_label("Show Log");
    check_enable_log.set_checked(config.enable_log);

    let mut but_toggle = Button::default()
        .with_pos(110, 164)
        .with_size(width, 32)
        .with_label("Start");

    // Project Links (Upstream & Fork)
    let mut link_sep = Frame::default()
        .with_pos(20, 204)
        .with_size(270, 1);
    link_sep.set_frame(FrameType::EngravedFrame);

    let mut but_upstream = Button::default()
        .with_pos(20, 212)
        .with_size(270, 22)
        .with_label("🔗 Upstream: github.com/H-M-H/Weylus");
    but_upstream.set_frame(FrameType::FlatBox);
    but_upstream.set_label_size(11);
    but_upstream.set_label_color(Color::from_rgb(37, 99, 235));
    but_upstream.set_tooltip("https://github.com/H-M-H/Weylus");
    but_upstream.set_callback(|_| {
        open_url("https://github.com/H-M-H/Weylus");
    });

    let mut but_fork = Button::default()
        .with_pos(20, 238)
        .with_size(270, 22)
        .with_label("🔗 Fork: github.com/LycheeMiku/Weylus-lm39");
    but_fork.set_frame(FrameType::FlatBox);
    but_fork.set_label_size(11);
    but_fork.set_label_color(Color::from_rgb(37, 99, 235));
    but_fork.set_tooltip("https://github.com/LycheeMiku/Weylus-lm39");
    but_fork.set_callback(|_| {
        open_url("https://github.com/LycheeMiku/Weylus-lm39");
    });

    // Right Column: QR Code & Connection Info
    let mut qr_frame = Frame::default()
        .with_pos(340, 16)
        .with_size(210, 210);
    qr_frame.set_frame(FrameType::BorderFrame);

    let output_server_addr = Output::default()
        .with_pos(340, 234)
        .with_size(210, 26);

    let mut but_copy_url = Button::default()
        .with_pos(558, 234)
        .with_size(72, 26)
        .with_label("Copy");

    let mut status_frame = Frame::default()
        .with_pos(340, 268)
        .with_size(290, 22)
        .with_label("● Server Stopped");
    status_frame.set_label_color(Color::from_rgb(160, 160, 160));
    status_frame.set_label_size(12);

    // Closure to pin all top panel widgets to their exact pixel coordinates
    let mut reset_top_widgets = {
        let mut input_access_code = input_access_code.clone();
        let mut input_bind_addr = input_bind_addr.clone();
        let mut input_port = input_port.clone();
        let mut choice_theme = choice_theme.clone();
        let mut check_auto_start = check_auto_start.clone();
        let mut check_enable_log = check_enable_log.clone();
        let mut but_toggle = but_toggle.clone();
        let mut link_sep = link_sep.clone();
        let mut but_upstream = but_upstream.clone();
        let mut but_fork = but_fork.clone();
        let mut qr_frame = qr_frame.clone();
        let mut output_server_addr = output_server_addr.clone();
        let mut but_copy_url = but_copy_url.clone();
        let mut status_frame = status_frame.clone();

        move || {
            input_access_code.resize(110, 16, 170, 24);
            input_bind_addr.resize(110, 46, 170, 24);
            input_port.resize(110, 76, 170, 24);
            choice_theme.resize(110, 106, 170, 24);
            check_auto_start.resize(110, 136, 90, 22);
            check_enable_log.resize(205, 136, 85, 22);
            but_toggle.resize(110, 164, 170, 32);
            link_sep.resize(20, 204, 270, 1);
            but_upstream.resize(20, 212, 270, 22);
            but_fork.resize(20, 238, 270, 22);
            qr_frame.resize(340, 16, 210, 210);
            output_server_addr.resize(340, 234, 210, 26);
            but_copy_url.resize(558, 234, 72, 26);
            status_frame.resize(340, 268, 290, 22);
        }
    };

    // Bottom: Log output with color highlighting
    let output_buf = TextBuffer::default();
    let style_buf = TextBuffer::default();
    let mut output = TextDisplay::default()
        .with_pos(20, 304)
        .with_size(620, 220);

    let style_table = vec![
        // 'A': Default
        StyleTableEntry {
            color: Color::from_rgb(228, 228, 231),
            font: Font::Courier,
            size: 12,
        },
        // 'B': DEBUG (Sky Blue)
        StyleTableEntry {
            color: Color::from_rgb(96, 165, 250),
            font: Font::CourierBold,
            size: 12,
        },
        // 'C': INFO (Soft Green)
        StyleTableEntry {
            color: Color::from_rgb(74, 222, 128),
            font: Font::Courier,
            size: 12,
        },
        // 'D': WARN (Amber / Orange)
        StyleTableEntry {
            color: Color::from_rgb(251, 191, 36),
            font: Font::CourierBold,
            size: 12,
        },
        // 'E': ERROR (Bright Red)
        StyleTableEntry {
            color: Color::from_rgb(248, 113, 113),
            font: Font::CourierBold,
            size: 12,
        },
    ];

    output.set_buffer(output_buf);
    output.set_highlight_data(style_buf.clone(), style_table);
    output.set_color(Color::from_rgb(24, 24, 27));
    output.set_text_color(Color::from_rgb(228, 228, 231));
    output.set_text_font(Font::Courier);
    output.set_text_size(12);
    output.wrap_mode(fltk::text::WrapMode::AtBounds, 0);

    if !config.enable_log {
        output.hide();
    }

    let output_buf = output.buffer().unwrap();

    wind.resizable(&output);
    wind.end();
    wind.show();
    reset_top_widgets();
    wind.init_sizes();

    let is_log_enabled = Arc::new(std::sync::atomic::AtomicBool::new(config.enable_log));
    let log_bufs = Arc::new(Mutex::new((output_buf, style_buf)));

    {
        let is_log_enabled = is_log_enabled.clone();
        let log_bufs = log_bufs.clone();
        std::thread::spawn(move || {
            while let Ok(log_message) = log_receiver.recv() {
                if is_log_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                    let style_char = if log_message.contains("ERROR") || log_message.contains("error") {
                        'E'
                    } else if log_message.contains("WARN") || log_message.contains("warn") {
                        'D'
                    } else if log_message.contains("DEBUG") || log_message.contains("debug") {
                        'B'
                    } else if log_message.contains("INFO") || log_message.contains("info") {
                        'C'
                    } else {
                        'A'
                    };
                    let style_line = style_char.to_string().repeat(log_message.len());
                    let mut bufs = log_bufs.lock().unwrap();
                    bufs.1.append(&style_line);
                    bufs.0.append(&log_message);
                }
            }
        });
    }

    let mut weylus = crate::weylus::Weylus::new();
    let auto_start = config.auto_start;
    let current_url = Arc::new(Mutex::new(String::new()));
    let config = Arc::new(Mutex::new(config.clone()));

    {
        let config = config.clone();
        let mut wind = wind.clone();
        let mut reset_top = reset_top_widgets.clone();
        choice_theme.set_callback(move |c| {
            let v = c.value();
            if v >= 0 {
                ThemeType::from_index(v).apply();
                config.lock().unwrap().gui_theme = Some(ThemeType::from_index(v));
                write_config(&config.lock().unwrap());
                reset_top();
                wind.redraw();
            }
        });
    }

    {
        let is_log_enabled = is_log_enabled.clone();
        let log_bufs = log_bufs.clone();
        let mut output = output.clone();
        let mut wind = wind.clone();
        let config = config.clone();
        let mut reset_top = reset_top_widgets.clone();

        check_enable_log.set_callback(move |btn| {
            let enabled = btn.is_checked();
            is_log_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
            {
                let mut cfg = config.lock().unwrap();
                cfg.enable_log = enabled;
                write_config(&cfg);
            }
            if enabled {
                wind.set_size(660, 536);
                wind.size_range(660, 536, 660, 0);
                output.resize(20, 304, 620, 220);
                output.show();
                reset_top();
                wind.resizable(&output);
                wind.init_sizes();
                wind.redraw();
            } else {
                {
                    let mut bufs = log_bufs.lock().unwrap();
                    bufs.0.set_text("");
                    bufs.1.set_text("");
                }
                output.hide();
                wind.set_size(660, 304);
                wind.size_range(660, 304, 660, 304);
                reset_top();
                wind.init_sizes();
                wind.redraw();
            }
        });
    }

    {
        let current_url = current_url.clone();
        let mut but_copy_url_clone = but_copy_url.clone();
        but_copy_url.set_callback(move |_| {
            let url = current_url.lock().unwrap().clone();
            if !url.is_empty() {
                fltk::app::copy(&url);
                but_copy_url_clone.set_label("Copied!");
                let mut btn = but_copy_url_clone.clone();
                fltk::app::add_timeout3(1.5, move |_| {
                    btn.set_label("Copy");
                });
            }
        });
    }

    let mut toggle_server = {
        let current_url = current_url.clone();
        let mut status_frame = status_frame.clone();
        let mut output_server_addr = output_server_addr.clone();
        let mut qr_frame = qr_frame.clone();
        let is_server_running = is_server_running.clone();
        let wind = wind.clone();

        move |but: &mut Button| {
            if let Err(err) = || -> Result<(), Box<dyn std::error::Error>> {
                let mut config = config.lock().unwrap();
                if !is_server_running.load(std::sync::atomic::Ordering::SeqCst) {
                    let access_code_string = input_access_code.value();
                    let access_code = match access_code_string.as_str() {
                        "" => None,
                        code => Some(code),
                    };
                    let bind_addr: IpAddr = input_bind_addr.value().parse()?;
                    let web_port: u16 = input_port.value().parse()?;

                    config.access_code = access_code.map(|s| s.to_string());
                    config.web_port = web_port;
                    config.bind_address = bind_addr;
                    config.auto_start = check_auto_start.is_checked();
                    config.enable_log = check_enable_log.is_checked();
                    config.gui_theme = Some(ThemeType::from_index(choice_theme.value()));

                    let status_frame_clone = status_frame.clone();

                    if !weylus.start(&config, move |message| match message {
                        Web2UiMessage::UInputInaccessible => awake_callback(move || {
                            let w = 500;
                            let h = 300;
                            let mut pop_up = Window::default()
                                .with_size(w, h)
                                .center_screen()
                                .with_label("Weylus - UInput inaccessible!");
                            pop_up.set_xclass("weylus");

                            let buf = TextBuffer::default();
                            let mut pop_up_text = TextDisplay::default().with_size(w, h);
                            pop_up_text.set_buffer(buf);
                            pop_up_text.wrap_mode(fltk::text::WrapMode::AtBounds, 5);
                            let mut buf = pop_up_text.buffer().unwrap();
                            buf.set_text(std::include_str!("strings/uinput_error.txt"));

                            pop_up.end();
                            pop_up.make_modal(true);
                            pop_up.show();
                        }),
                        Web2UiMessage::ClientConnected(client_info) => {
                            let mut sf = status_frame_clone.clone();
                            awake_callback(move || {
                                sf.set_label(&format!("● Connected: {}", client_info));
                                sf.set_label_color(Color::from_rgb(34, 197, 94));
                                sf.redraw();
                            });
                        }
                        Web2UiMessage::ClientDisconnected => {
                            let mut sf = status_frame_clone.clone();
                            awake_callback(move || {
                                sf.set_label("● Running: Ready for Tablet");
                                sf.set_label_color(Color::from_rgb(59, 130, 246));
                                sf.redraw();
                            });
                        }
                    }) {
                        return Ok(());
                    }
                    is_server_running.store(true, std::sync::atomic::Ordering::SeqCst);

                    write_config(&config);

                    let lan_ip = get_lan_ip(config.bind_address);
                    let mut url_string = format!("http://{}:{}", lan_ip, config.web_port);
                    if let Some(access_code) = &config.access_code {
                        url_string.push_str("?access_code=");
                        url_string.push_str(
                            &percent_encoding::utf8_percent_encode(
                                access_code,
                                percent_encoding::NON_ALPHANUMERIC,
                            )
                            .to_string(),
                        );
                    }

                    *current_url.lock().unwrap() = url_string.clone();
                    output_server_addr.set_value(&url_string);
                    render_qr_to_frame(&mut qr_frame, &url_string, 210);

                    status_frame.set_label("● Running: Ready for Tablet");
                    status_frame.set_label_color(Color::from_rgb(59, 130, 246));
                    status_frame.redraw();

                    but.set_label("Stop");
                    info!("Weylus Graphics Tablet server started on {}", url_string);
                } else {
                    if !confirm_dialog(&wind, "Stop Server Confirmation", "Server is currently running. Are you sure you want to stop it?") {
                        return Ok(());
                    }
                    weylus.stop();
                    but.set_label("Start");
                    qr_frame.set_image(None::<PngImage>);
                    qr_frame.redraw();
                    output_server_addr.set_value("");
                    status_frame.set_label("● Server Stopped");
                    status_frame.set_label_color(Color::from_rgb(160, 160, 160));
                    status_frame.redraw();
                    *current_url.lock().unwrap() = String::new();
                    is_server_running.store(false, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(())
            }() {
                error!("{}", err);
            };
        }
    };

    if auto_start {
        toggle_server(&mut but_toggle);
    }

    but_toggle.set_callback(toggle_server);

    app.run().expect("Failed to run Gui!");
    but_toggle.set_callback(|_| ());
}

const BORDER: i32 = 30;
static WINCTX: Mutex<Option<InputAreaWindowContext>> = Mutex::new(None);

struct InputAreaWindowContext {
    win: Window,
    choice_mouse: Choice,
    choice_touch: Choice,
    choice_pen: Choice,
    workspaces: Vec<Rect>,
}

pub fn get_input_area(
    no_gui: bool,
    output_sender: std::sync::mpsc::Sender<crate::protocol::CustomInputAreas>,
) {
    if no_gui {
        static GUI_INITIALIZED: AtomicBool = AtomicBool::new(false);

        if !GUI_INITIALIZED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            std::thread::spawn(move || {
                let _app = App::default().with_scheme(fltk::app::AppScheme::Gtk);
                let mut winctx = create_custom_input_area_window();
                custom_input_area_window_handle_events(&mut winctx.win, output_sender.clone());
                show_overlay_window(&mut winctx);
                WINCTX.lock().unwrap().replace(winctx);
                loop {
                    if let Err(err) = fltk::app::wait_for(1.0) {
                        warn!("Error waiting for fltk events: {err}.");
                    }
                }
            });
        } else {
            fltk::app::awake_callback(move || {
                let mut winctx = WINCTX.lock().unwrap();
                let winctx = winctx.as_mut().unwrap();
                custom_input_area_window_handle_events(&mut winctx.win, output_sender.clone());
                show_overlay_window(winctx);
            });
        }
    } else {
        fltk::app::awake_callback(move || {
            let mut winctx = WINCTX.lock().unwrap();
            if winctx.is_none() {
                winctx.replace(create_custom_input_area_window());
            }

            let winctx = winctx.as_mut().unwrap();
            custom_input_area_window_handle_events(&mut winctx.win, output_sender.clone());
            show_overlay_window(winctx);
        });
    }
}

fn create_custom_input_area_window() -> InputAreaWindowContext {
    let mut win = Window::default().with_size(600, 600).center_screen();
    win.make_resizable(true);
    win.set_border(false);
    win.set_frame(FrameType::FlatBox);
    win.set_color(Color::from_rgb(240, 240, 240));
    let mut frame = Frame::default()
        .with_size(win.w() - 2 * BORDER, win.h() - 2 * BORDER)
        .center_of_parent()
        .with_label("Press Enter to submit,\nEscape to abort.");
    frame.set_label_type(LabelType::Normal);
    frame.set_label_size(20);
    frame.set_color(Color::Black);
    frame.set_frame(FrameType::BorderFrame);
    frame.set_label_font(fltk::enums::Font::HelveticaBold);

    let width = 200;
    let height = 30;
    let padding = 10;
    let choice_mouse = Choice::default()
        .with_size(width, height)
        .with_pos(padding, 4 * padding)
        .center_x(&frame)
        .with_id("choice_mouse")
        .with_label("Map Mouse from:");
    let choice_touch = Choice::default()
        .with_size(width, height)
        .below_of(&choice_mouse, padding)
        .with_id("choice_touch")
        .with_label("Map Touch from:");
    let choice_pen = Choice::default()
        .with_size(width, height)
        .below_of(&choice_touch, padding)
        .with_id("choice_pen")
        .with_label("Map Pen from:");

    frame.handle(|frame, event| match event {
        fltk::enums::Event::Push => {
            if fltk::app::event_clicks() {
                if let Some(mut win) = frame.window() {
                    win.fullscreen(!win.fullscreen_active());
                }
                true
            } else {
                false
            }
        }
        _ => false,
    });
    win.resize_callback(move |_win, _x, _y, w, h| {
        frame.resize(BORDER, BORDER, w - 2 * BORDER, h - 2 * BORDER)
    });
    win.end();
    InputAreaWindowContext {
        win,
        choice_mouse,
        choice_touch,
        choice_pen,
        workspaces: Vec::new(),
    }
}

fn custom_input_area_window_handle_events(
    win: &mut Window,
    sender: std::sync::mpsc::Sender<crate::protocol::CustomInputAreas>,
) {
    #[derive(Debug)]
    enum MouseFlags {
        All,
        Edge(bool, bool),
        Corner(bool, bool),
    }
    fn get_mouse_flags(win: &Window, x: i32, y: i32) -> MouseFlags {
        let dx0 = (win.x() - x).abs();
        let dy0 = (win.y() - y).abs();
        let dx1 = (win.x() + win.w() - x).abs();
        let dy1 = (win.y() + win.h() - y).abs();
        let dx = min(dx0, dx1);
        let dy = min(dy0, dy1);
        let d = min(dx, dy);
        if d <= BORDER {
            if dx <= BORDER && dy <= BORDER {
                MouseFlags::Corner(dx0 <= dx1, dy0 <= dy1)
            } else {
                MouseFlags::Edge(dx <= dy, if dx <= dy { dx0 <= dx1 } else { dy0 <= dy1 })
            }
        } else {
            MouseFlags::All
        }
    }
    fn get_screen_coords_from_event_coords(win: &Window, (x, y): (i32, i32)) -> (i32, i32) {
        (x + win.x(), y + win.y())
    }
    fn set_cursor(
        win: &mut Window,
        current_cursor: &mut fltk::enums::Cursor,
        flags: Option<MouseFlags>,
    ) {
        let cursor = match flags {
            Some(MouseFlags::All) => fltk::enums::Cursor::Move,
            Some(MouseFlags::Edge(bx, by)) => match (bx, by) {
                (true, true) => fltk::enums::Cursor::W,
                (true, false) => fltk::enums::Cursor::E,
                (false, true) => fltk::enums::Cursor::N,
                (false, false) => fltk::enums::Cursor::S,
            },
            Some(MouseFlags::Corner(bx, by)) => match (bx, by) {
                (true, true) => fltk::enums::Cursor::NWSE,
                (true, false) => fltk::enums::Cursor::NESW,
                (false, true) => fltk::enums::Cursor::NESW,
                (false, false) => fltk::enums::Cursor::NWSE,
            },
            None => fltk::enums::Cursor::Default,
        };
        if *current_cursor != cursor {
            *current_cursor = cursor;
            win.set_cursor(cursor);
        }
    }

    let mut drag_flags = MouseFlags::All;
    let mut current_cursor = fltk::enums::Cursor::Default;
    let mut x = 0;
    let mut y = 0;
    let mut win_x_drag_start = 0;
    let mut win_y_drag_start = 0;
    let mut win_w_drag_start = 0;
    let mut win_h_drag_start = 0;
    win.handle(move |win, event| {
        match event {
            fltk::enums::Event::Move => {
                let (x, y) = get_screen_coords_from_event_coords(&win, fltk::app::event_coords());
                let flags = get_mouse_flags(&win, x, y);
                set_cursor(win, &mut current_cursor, Some(flags));
                true
            }
            fltk::enums::Event::Leave => {
                win.set_cursor(fltk::enums::Cursor::Default);
                true
            }
            fltk::enums::Event::Push => {
                (x, y) = get_screen_coords_from_event_coords(&win, fltk::app::event_coords());
                win_x_drag_start = win.x();
                win_y_drag_start = win.y();
                win_w_drag_start = win.w();
                win_h_drag_start = win.h();
                drag_flags = get_mouse_flags(&win, x, y);
                true
            }
            fltk::enums::Event::Drag => {
                if win.opacity() == 1.0 {
                    win.set_opacity(0.5);
                }
                let (x_new, y_new) = get_screen_coords_from_event_coords(&win, fltk::app::event_coords());
                let dx = x_new - x;
                let dy = y_new - y;
                match drag_flags {
                    MouseFlags::All => win.set_pos(win_x_drag_start + dx, win_y_drag_start + dy),
                    MouseFlags::Edge(bx, by) => match (bx, by) {
                        (true, true) => win.resize(
                            win_x_drag_start + dx,
                            win_y_drag_start,
                            win_w_drag_start - dx,
                            win_h_drag_start,
                        ),
                        (true, false) => win.resize(
                            win_x_drag_start,
                            win_y_drag_start,
                            win_w_drag_start + dx,
                            win_h_drag_start,
                        ),
                        (false, true) => win.resize(
                            win_x_drag_start,
                            win_y_drag_start + dy,
                            win_w_drag_start,
                            win_h_drag_start - dy,
                        ),
                        (false, false) => win.resize(
                            win_x_drag_start,
                            win_y_drag_start,
                            win_w_drag_start,
                            win_h_drag_start + dy,
                        ),
                    },
                    MouseFlags::Corner(bx, by) => match (bx, by) {
                        (true, true) => win.resize(
                            win_x_drag_start + dx,
                            win_y_drag_start + dy,
                            win_w_drag_start - dx,
                            win_h_drag_start - dy,
                        ),

                        (true, false) => win.resize(
                            win_x_drag_start + dx,
                            win_y_drag_start,
                            win_w_drag_start - dx,
                            win_h_drag_start + dy,
                        ),
                        (false, true) => win.resize(
                            win_x_drag_start,
                            win_y_drag_start + dy,
                            win_w_drag_start + dx,
                            win_h_drag_start - dy,
                        ),
                        (false, false) => win.resize(
                            win_x_drag_start,
                            win_y_drag_start,
                            win_w_drag_start + dx,
                            win_h_drag_start + dy,
                        ),
                    },
                }
                true
            }
            fltk::enums::Event::Released => {
                if win.opacity() != 1.0 {
                    win.set_opacity(1.0);
                }
                true
            }
            fltk::enums::Event::KeyDown => match fltk::app::event_key() {
                fltk::enums::Key::Enter => {
                    fn relative_rect(win: &Window, workspace: &Rect) -> Rect {
                        let mut rect = crate::protocol::Rect {
                            x: (win.x() as f64 - workspace.x).min(workspace.w) / workspace.w,
                            y: (win.y() as f64 - workspace.y).min(workspace.h) / workspace.h,
                            w: win.w().max(1) as f64 / workspace.w,
                            h: win.h().max(1) as f64 / workspace.h,
                        };
                        rect.w = rect.w.min(1.0 - rect.x);
                        rect.h = rect.h.min(1.0 - rect.y);
                        rect
                    }
                    win.set_cursor(fltk::enums::Cursor::Default);
                    win.hide();
                    let mut areas = CustomInputAreas::default();
                    let workspaces = WINCTX.lock().unwrap().as_ref().unwrap().workspaces.clone();
                    for (name, area) in [
                        ("choice_mouse", &mut areas.mouse),
                        ("choice_touch", &mut areas.touch),
                        ("choice_pen", &mut areas.pen),
                    ] {
                        let c: Choice = fltk::app::widget_from_id(name).unwrap();
                        match c.value() {
                            0 => (),
                            v @ 1.. if (v as usize) <= workspaces.len() => {
                                let workspace = workspaces[v as usize - 1];
                                *area = Some(relative_rect(win, &workspace))
                            }
                            v => warn!("Unexpected value in {name}: {v}!"),
                        }
                    }
                    let _ = sender.send(areas);
                    true
                }
                fltk::enums::Key::Escape => {
                    win.set_cursor(fltk::enums::Cursor::Default);
                    win.hide();
                    true
                }
                _ => false,
            },
            _ => false,
        }
    });
}

fn show_overlay_window(winctx: &mut InputAreaWindowContext) {
    let win = &mut winctx.win;
    if win.shown() {
        return;
    }
    let screens = fltk::app::Screen::all_screens();
    winctx.workspaces.clear();
    winctx.workspaces.push(get_full_workspace_rect());
    for screen in &screens {
        let fltk::draw::Rect { x, y, w, h } = screen.work_area();
        winctx.workspaces.push(Rect {
            x: x as f64,
            y: y as f64,
            w: w as f64,
            h: h as f64,
        });
    }
    for c in [
        &mut winctx.choice_mouse,
        &mut winctx.choice_touch,
        &mut winctx.choice_pen,
    ] {
        let v = c.value();
        c.clear();
        c.add_choice("None");
        c.add_choice("Full Workspace");
        for screen in &screens {
            c.add_choice(&format!(
                "Screen {n} at {w}x{h}+{x}+{y}",
                n = screen.n,
                w = screen.w(),
                h = screen.h(),
                x = screen.x(),
                y = screen.y()
            ));
        }
        if v >= 0 && (v as usize) < 2 + screens.len() {
            c.set_value(v);
        } else {
            c.set_value(0);
        }
    }
    if win.fullscreen_active() {
        win.set_size(600, 600);
        let n = win.screen_num();
        let screen = fltk::app::Screen::new(n).unwrap();
        win.set_pos(
            screen.x() + (screen.w() - 600) / 2,
            screen.y() + (screen.h() - 600) / 2,
        );
    }
    win.show();
    win.set_on_top();
    win.set_visible_focus();
}

pub fn get_full_workspace_rect() -> Rect {
    let mut rect = Rect::default();
    for screen in fltk::app::Screen::all_screens() {
        let fltk::draw::Rect { x, y, w, h } = screen.work_area();
        rect.x = (x as f64).min(rect.x);
        rect.y = (y as f64).min(rect.y);
        rect.w = ((x + w) as f64).max(rect.w);
        rect.h = ((y + h) as f64).max(rect.h);
    }
    rect
}
