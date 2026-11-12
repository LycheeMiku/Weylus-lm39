use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, WebSocket, WebSocketError};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::sync::{mpsc, Arc};
use std::thread::{spawn, JoinHandle};
use tokio::sync::mpsc::channel;
use tracing::{error, trace, warn};

use crate::capturable::{get_capturables, Capturable};
use crate::input::device::InputDevice;
use crate::protocol::{
    Button, ClientConfiguration, KeyboardEvent, MessageInbound, MessageOutbound, PointerEvent,
    PointerEventType, PointerType, WeylusReceiver, WeylusSender, WheelEvent,
};

#[allow(dead_code)]
struct VideoConfig {
    capturable: Box<dyn Capturable>,
    capture_cursor: bool,
    max_width: usize,
    max_height: usize,
    frame_rate: f64,
}

enum VideoCommands {
    Start(VideoConfig),
    Pause,
    Resume,
    Restart,
}

fn send_message<S>(sender: &mut S, message: MessageOutbound)
where
    S: WeylusSender,
{
    if let Err(err) = sender.send_message(message) {
        warn!("Failed to send message to client: {err}");
    }
}

pub struct WeylusClientHandler<S, R, FnUInput> {
    sender: S,
    receiver: Option<R>,
    video_sender: mpsc::Sender<VideoCommands>,
    input_device: Option<Box<dyn InputDevice>>,
    capturables: Vec<Box<dyn Capturable>>,
    on_uinput_inaccessible: FnUInput,
    config: WeylusClientConfig,
    #[cfg(target_os = "linux")]
    capture_cursor: bool,
    client_name: Option<String>,
    video_thread: JoinHandle<()>,
}

#[derive(Clone, Copy)]
pub struct WeylusClientConfig {
    pub encoder_options: crate::video::EncoderOptions,
    #[cfg(target_os = "linux")]
    pub wayland_support: bool,
    pub no_gui: bool,
}

impl<S, R, FnUInput> WeylusClientHandler<S, R, FnUInput> {
    pub fn new(
        sender: S,
        receiver: R,
        on_uinput_inaccessible: FnUInput,
        config: WeylusClientConfig,
    ) -> Self
    where
        R: WeylusReceiver,
        S: WeylusSender + Clone + Send + Sync + 'static,
    {
        let (video_sender, video_receiver) = mpsc::channel::<VideoCommands>();
        let video_thread = {
            let sender = sender.clone();
            spawn(move || handle_video(video_receiver, sender, config.encoder_options))
        };

        Self {
            sender,
            receiver: Some(receiver),
            video_sender,
            input_device: None,
            capturables: vec![],
            on_uinput_inaccessible,
            config,
            #[cfg(target_os = "linux")]
            capture_cursor: false,
            client_name: None,
            video_thread,
        }
    }

    pub fn run(mut self)
    where
        R: WeylusReceiver,
        S: WeylusSender + Clone + Send + Sync + 'static,
        FnUInput: Fn(),
    {
        for message in self.receiver.take().unwrap() {
            match message {
                Ok(message) => {
                    trace!("Received message: {message:?}");
                    match message {
                        MessageInbound::PointerEvent(event) => self.process_pointer_event(&event),
                        MessageInbound::WheelEvent(event) => self.process_wheel_event(&event),
                        MessageInbound::KeyboardEvent(event) => self.process_keyboard_event(&event),
                        MessageInbound::GetCapturableList => self.send_capturable_list(),
                        MessageInbound::Config(config) => self.update_config(config),
                        MessageInbound::PauseVideo => {
                            let _ = self.video_sender.send(VideoCommands::Pause);
                        }
                        MessageInbound::ResumeVideo => {
                            let _ = self.video_sender.send(VideoCommands::Resume);
                        }
                        MessageInbound::RestartVideo => {
                            let _ = self.video_sender.send(VideoCommands::Restart);
                        }
                        MessageInbound::ChooseCustomInputAreas => {
                            let (sender, receiver) = std::sync::mpsc::channel();
                            crate::gui::get_input_area(self.config.no_gui, sender);
                            let mut sender = self.sender.clone();
                            spawn(move || {
                                while let Ok(areas) = receiver.recv() {
                                    send_message(
                                        &mut sender,
                                        MessageOutbound::CustomInputAreas(areas),
                                    );
                                }
                            });
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to read message {err}!");
                    self.send_message(MessageOutbound::Error(
                        "Failed to read message!".to_string(),
                    ));
                }
            }
        }
        drop(self.video_sender);
        if let Err(err) = self.video_thread.join() {
            warn!("Failed to join video thread: {err:?}");
        }
    }

    fn send_message(&mut self, message: MessageOutbound)
    where
        S: WeylusSender,
    {
        send_message(&mut self.sender, message)
    }

    fn process_wheel_event(&mut self, event: &WheelEvent) {
        match &mut self.input_device {
            Some(i) => i.send_wheel_event(event),
            None => warn!("Input device is not initialized, cannot process WheelEvent!"),
        }
    }

    fn process_pointer_event(&mut self, event: &PointerEvent) {
        if let Some(i) = self.input_device.as_mut() {
            i.send_pointer_event(event);
        } else {
            // Lazy init input device with default capturable if not already initialized
            if self.capturables.is_empty() {
                self.capturables = get_capturables(
                    #[cfg(target_os = "linux")]
                    self.config.wayland_support,
                    #[cfg(target_os = "linux")]
                    self.capture_cursor,
                );
            }
            if !self.capturables.is_empty() {
                let capturable = self.capturables[0].clone();
                #[cfg(target_os = "windows")]
                {
                    let mut dev = crate::input::autopilot_device_win::WindowsInput::new(capturable);
                    dev.send_pointer_event(event);
                    self.input_device = Some(Box::new(dev));
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let mut dev = crate::input::autopilot_device::AutoPilotDevice::new(capturable);
                    dev.send_pointer_event(event);
                    self.input_device = Some(Box::new(dev));
                }
            } else {
                warn!("No display capturable found, cannot process PointerEvent!");
            }
        }
    }

    fn process_keyboard_event(&mut self, event: &KeyboardEvent) {
        if let Some(i) = self.input_device.as_mut() {
            i.send_keyboard_event(event);
        } else {
            warn!("Input device is not initialized, cannot process KeyboardEvent!");
        }
    }

    fn send_capturable_list(&mut self)
    where
        S: WeylusSender,
    {
        let mut windows = Vec::<String>::new();
        self.capturables = get_capturables(
            #[cfg(target_os = "linux")]
            self.config.wayland_support,
            #[cfg(target_os = "linux")]
            self.capture_cursor,
        );
        self.capturables.iter().for_each(|c| {
            windows.push(c.name());
        });
        self.send_message(MessageOutbound::CapturableList(windows));
    }

    fn update_config(&mut self, config: ClientConfiguration)
    where
        S: WeylusSender,
        FnUInput: Fn(),
    {
        let _client_name_changed = if self.client_name != config.client_name {
            self.client_name = config.client_name.clone();
            true
        } else {
            false
        };

        if self.capturables.is_empty() {
            self.capturables = get_capturables(
                #[cfg(target_os = "linux")]
                self.config.wayland_support,
                #[cfg(target_os = "linux")]
                self.capture_cursor,
            );
        }

        let capturable_id = if config.capturable_id < self.capturables.len() {
            config.capturable_id
        } else {
            0
        };

        if capturable_id < self.capturables.len() {
            let capturable = self.capturables[capturable_id].clone();

            #[cfg(target_os = "linux")]
            {
                self.capture_cursor = config.capture_cursor;
            }

            #[cfg(target_os = "linux")]
            if config.uinput_support {
                if self.input_device.as_ref().map_or(true, |d| {
                    client_name_changed || d.device_type() != InputDeviceType::UInputDevice
                }) {
                    let device = crate::input::uinput_device::UInputDevice::new(
                        capturable.clone(),
                        &self.client_name,
                    );
                    match device {
                        Ok(d) => self.input_device = Some(Box::new(d)),
                        Err(e) => {
                            error!("Failed to create uinput device: {}", e);
                            if let crate::cerror::CErrorCode::UInputNotAccessible = e.to_enum() {
                                (self.on_uinput_inaccessible)();
                            }
                            self.send_message(MessageOutbound::ConfigError(
                                "Failed to create uinput device!".to_string(),
                            ));
                            return;
                        }
                    }
                } else if let Some(d) = self.input_device.as_mut() {
                    d.set_capturable(capturable.clone());
                    d.update_tablet_config(config.stylus_only, config.orientation, config.pressure_sensitivity);
                }
            } else if self.input_device.as_ref().map_or(true, |d| {
                d.device_type() != InputDeviceType::AutoPilotDevice
            }) {
                self.input_device = Some(Box::new(
                    crate::input::autopilot_device::AutoPilotDevice::new(capturable.clone()),
                ));
            } else if let Some(d) = self.input_device.as_mut() {
                d.set_capturable(capturable.clone());
                d.update_tablet_config(config.stylus_only, config.orientation, config.pressure_sensitivity);
            }

            #[cfg(target_os = "macos")]
            if self.input_device.is_none() {
                self.input_device = Some(Box::new(
                    crate::input::autopilot_device::AutoPilotDevice::new(capturable.clone()),
                ));
            } else {
                self.input_device
                    .as_mut()
                    .map(|d| d.set_capturable(capturable.clone()));
            }

            #[cfg(target_os = "windows")]
            if self.input_device.is_none() {
                let mut dev = crate::input::autopilot_device_win::WindowsInput::new(capturable.clone());
                dev.update_tablet_config(config.stylus_only, config.orientation, config.pressure_sensitivity);
                self.input_device = Some(Box::new(dev));
            } else if let Some(d) = self.input_device.as_mut() {
                d.set_capturable(capturable.clone());
                d.update_tablet_config(config.stylus_only, config.orientation, config.pressure_sensitivity);
            }

            if config.tablet_mode {
                // Pure graphics tablet mode: no screen capture loop, no video stream encoding
                self.send_message(MessageOutbound::ConfigOk);
            } else {
                self.video_sender
                    .send(VideoCommands::Start(VideoConfig {
                        capturable,
                        capture_cursor: config.capture_cursor,
                        max_width: config.max_width,
                        max_height: config.max_height,
                        frame_rate: config.frame_rate,
                    }))
                    .unwrap();
            }
        } else {
            error!("Got invalid id for capturable: {}", config.capturable_id);
            self.send_message(MessageOutbound::ConfigError(
                "Invalid id for capturable!".to_string(),
            ));
        }
    }
}

fn handle_video<S: WeylusSender + Clone + 'static>(
    receiver: mpsc::Receiver<VideoCommands>,
    mut sender: S,
    _encoder_options: crate::video::EncoderOptions,
) {
    // In tablet mode, video capturing and encoding is stripped to save CPU/GPU and network bandwidth.
    while let Ok(cmd) = receiver.recv() {
        match cmd {
            VideoCommands::Start(_) => {
                send_message(&mut sender, MessageOutbound::ConfigOk);
            }
            _ => {}
        }
    }
}

pub struct WsWeylusReceiver {
    recv: tokio::sync::mpsc::Receiver<MessageInbound>,
}

impl Iterator for WsWeylusReceiver {
    type Item = Result<MessageInbound, Infallible>;

    fn next(&mut self) -> Option<Self::Item> {
        self.recv.blocking_recv().map(Ok)
    }
}

impl WeylusReceiver for WsWeylusReceiver {
    type Error = Infallible;
}

pub enum WsMessage {
    Frame(Frame<'static>),
    Video(Vec<u8>),
    MessageOutbound(MessageOutbound),
}

unsafe impl Send for WsMessage {}

#[derive(Clone)]
pub struct WsWeylusSender {
    sender: tokio::sync::mpsc::Sender<WsMessage>,
}

impl WeylusSender for WsWeylusSender {
    type Error = tokio::sync::mpsc::error::SendError<WsMessage>;

    fn send_message(&mut self, message: MessageOutbound) -> Result<(), Self::Error> {
        self.sender
            .blocking_send(WsMessage::MessageOutbound(message))
    }

    fn send_video(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.sender.blocking_send(WsMessage::Video(bytes.to_vec()))
    }
}

fn decode_binary_pointer_event(chunk: &[u8]) -> PointerEvent {
    let event_type = match chunk[0] {
        1 => PointerEventType::DOWN,
        2 => PointerEventType::UP,
        3 => PointerEventType::MOVE,
        4 => PointerEventType::CANCEL,
        5 => PointerEventType::ENTER,
        6 => PointerEventType::LEAVE,
        7 => PointerEventType::OVER,
        8 => PointerEventType::OUT,
        _ => PointerEventType::MOVE,
    };
    let pointer_type = match chunk[1] {
        1 => PointerType::Pen,
        2 => PointerType::Touch,
        3 => PointerType::Mouse,
        _ => PointerType::Unknown,
    };
    let pointer_id = u16::from_le_bytes([chunk[2], chunk[3]]) as i64;
    let buttons = Button::from_bits_truncate(chunk[4]);
    let button = Button::from_bits_truncate(chunk[5]);
    let x = (u16::from_le_bytes([chunk[6], chunk[7]]) as f64) / 65535.0;
    let y = (u16::from_le_bytes([chunk[8], chunk[9]]) as f64) / 65535.0;
    let pressure = (u16::from_le_bytes([chunk[10], chunk[11]]) as f64) / 65535.0;
    let tilt_x = (chunk[12] as i8) as i32;
    let tilt_y = (chunk[13] as i8) as i32;
    let timestamp_ms = u32::from_le_bytes([chunk[14], chunk[15], chunk[16], chunk[17]]) as u64;

    PointerEvent {
        event_type,
        pointer_id,
        timestamp: timestamp_ms * 1000,
        is_primary: true,
        pointer_type,
        button,
        buttons,
        x,
        y,
        pressure,
        tilt_x,
        tilt_y,
        twist: 0,
        width: 0.0,
        height: 0.0,
    }
}

pub fn weylus_websocket_channel(
    websocket: WebSocket<TokioIo<Upgraded>>,
    semaphore_shutdown: Arc<tokio::sync::Semaphore>,
) -> (WsWeylusSender, WsWeylusReceiver) {
    let (rx, mut tx) = websocket.split(|ws| tokio::io::split(ws));

    let mut rx = FragmentCollectorRead::new(rx);

    let (sender_inbound, receiver_inbound) = channel::<MessageInbound>(512);
    let (sender_outbound, mut receiver_outbound) = channel::<WsMessage>(512);

    {
        let sender_outbound = sender_outbound.clone();
        tokio::spawn(async move {
            let mut send_fn = |frame| async {
                if let Err(err) = sender_outbound.send(WsMessage::Frame(frame)).await {
                    warn!("Failed to send websocket frame while receiving fragmented frame: {err}.")
                };
                Ok(())
            };

            loop {
                let fut = rx.read_frame::<_, WebSocketError>(&mut send_fn);

                let frame = tokio::select! {
                    _ = semaphore_shutdown.acquire() => break,
                    frame = fut => match frame {
                        Ok(frame) => frame,
                        Err(err) => {
                            warn!("Invalid websocket frame: {err}.");
                            break;
                        },
                    },
                };
                match frame.opcode {
                    OpCode::Close => break,
                    OpCode::Text => match serde_json::from_slice(&frame.payload) {
                        Ok(msg) => {
                            if let Err(err) = sender_inbound.send(msg).await {
                                warn!("Failed to forward inbound message to WeylusClientHandler: {err}.");
                            }
                        }
                        Err(err) => warn!("Failed to parse message: {err}"),
                    },
                    OpCode::Binary => {
                        let payload = &frame.payload;
                        if payload.len() % 18 == 0 && !payload.is_empty() {
                            for chunk in payload.chunks_exact(18) {
                                let event = decode_binary_pointer_event(chunk);
                                if let Err(err) = sender_inbound.send(MessageInbound::PointerEvent(event)).await {
                                    warn!("Failed to forward binary pointer event to WeylusClientHandler: {err}.");
                                    break;
                                }
                            }
                        } else {
                            warn!("Received binary frame with unexpected length: {}", payload.len());
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    tokio::spawn(async move {
        loop {
            let msg = if let Some(msg) = receiver_outbound.recv().await {
                msg
            } else {
                break;
            };

            match msg {
                WsMessage::Frame(frame) => {
                    if let Err(err) = tx.write_frame(frame).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send frame: {err}");
                    }
                }
                WsMessage::Video(data) => {
                    if let Err(err) = tx.write_frame(Frame::binary(data.into())).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send video frame: {err}");
                    }
                }
                WsMessage::MessageOutbound(msg) => {
                    let json_string = serde_json::to_string(&msg).unwrap();
                    let data = json_string.as_bytes();
                    if let Err(err) = tx.write_frame(Frame::text(data.into())).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send outbound message: {err}");
                    }
                }
            }
        }
    });

    (
        WsWeylusSender {
            sender: sender_outbound,
        },
        WsWeylusReceiver {
            recv: receiver_inbound,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_binary_pointer_event() {
        let mut chunk = [0u8; 18];
        chunk[0] = 3; // MOVE
        chunk[1] = 1; // Pen
        chunk[2] = 42; // pointer_id = 42
        chunk[3] = 0;
        chunk[4] = 1; // buttons = PRIMARY
        chunk[5] = 1; // button = PRIMARY
        let x_u16 = 32768u16;
        chunk[6..8].copy_from_slice(&x_u16.to_le_bytes());
        let y_u16 = 16384u16;
        chunk[8..10].copy_from_slice(&y_u16.to_le_bytes());
        let p_u16 = 49151u16;
        chunk[10..12].copy_from_slice(&p_u16.to_le_bytes());
        chunk[12] = 15; // tilt_x = 15
        chunk[13] = (-20i8) as u8; // tilt_y = -20
        let ts_u32 = 123456u32;
        chunk[14..18].copy_from_slice(&ts_u32.to_le_bytes());

        let ev = decode_binary_pointer_event(&chunk);
        assert_eq!(ev.event_type, PointerEventType::MOVE);
        assert_eq!(ev.pointer_type, PointerType::Pen);
        assert_eq!(ev.pointer_id, 42);
        assert_eq!(ev.buttons, Button::PRIMARY);
        assert_eq!(ev.button, Button::PRIMARY);
        assert!((ev.x - 0.5).abs() < 0.001);
        assert!((ev.y - 0.25).abs() < 0.001);
        assert!((ev.pressure - 0.75).abs() < 0.001);
        assert_eq!(ev.tilt_x, 15);
        assert_eq!(ev.tilt_y, -20);
        assert_eq!(ev.timestamp, 123456000);
    }
}
