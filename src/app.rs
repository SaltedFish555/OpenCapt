use crate::{
    StartupMode, capture,
    config::{AppConfig, AppPaths},
    hotkey::RegisteredHotkey,
    output,
    overlay::{OverlaySession, OverlaySignal, PinnedCapture},
    pin::PinWindow,
    settings,
    tray::{TrayAction, TrayHandles},
};
use global_hotkey::GlobalHotKeyEvent;
use std::{
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, SystemTime},
};
use tao::{
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::windows::EventLoopBuilderExtWindows,
};
use tracing::{error, info, warn};
use tray_icon::menu::MenuEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Selecting,
    Capturing,
    Exiting,
}

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    HotKey(GlobalHotKeyEvent),
    TrayMenu(MenuEvent),
    Overlay(OverlaySignal),
    ConfigFileChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfigFileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

pub fn run(config: AppConfig, paths: AppPaths, startup_mode: StartupMode) -> ! {
    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    event_loop_builder.with_dpi_aware(true);
    let event_loop = event_loop_builder.build();
    install_event_forwarders(event_loop.create_proxy());
    let mut app = App::new(config, paths, startup_mode, event_loop.create_proxy());

    event_loop.run(move |event, _event_loop, control_flow| {
        app.handle_event(event, control_flow);
        *control_flow = app.next_control_flow();
    });
}

fn install_event_forwarders(proxy: EventLoopProxy<UserEvent>) {
    let hotkey_proxy = proxy.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        let _ = hotkey_proxy.send_event(UserEvent::HotKey(event));
    }));

    let tray_proxy = proxy;
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = tray_proxy.send_event(UserEvent::TrayMenu(event));
    }));
}

struct App {
    config: AppConfig,
    config_file_stamp: Option<ConfigFileStamp>,
    paths: AppPaths,
    state: AppState,
    startup_mode: StartupMode,
    event_proxy: EventLoopProxy<UserEvent>,
    tray: Option<TrayHandles>,
    hotkey: Option<RegisteredHotkey>,
    overlay: Option<OverlaySession>,
    pin_windows: Vec<PinWindow>,
    overlay_requested_on_startup: bool,
}

impl App {
    fn new(
        config: AppConfig,
        paths: AppPaths,
        startup_mode: StartupMode,
        event_proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        Self {
            config,
            config_file_stamp: config_file_stamp(&paths.config_file),
            paths,
            state: AppState::Idle,
            startup_mode: startup_mode.clone(),
            event_proxy,
            tray: None,
            hotkey: None,
            overlay: None,
            pin_windows: Vec::new(),
            overlay_requested_on_startup: matches!(startup_mode, StartupMode::OverlayTest),
        }
    }

    fn handle_event(&mut self, event: Event<'_, UserEvent>, control_flow: &mut ControlFlow) {
        match event {
            Event::NewEvents(StartCause::Init) => {
                self.initialize();
                if self.overlay_requested_on_startup {
                    self.overlay_requested_on_startup = false;
                    self.start_selection();
                }
            }
            Event::UserEvent(UserEvent::HotKey(event)) => self.handle_hotkey_event(event),
            Event::UserEvent(UserEvent::TrayMenu(event)) => {
                self.handle_tray_menu_event(control_flow, event)
            }
            Event::UserEvent(UserEvent::Overlay(signal)) => self.handle_overlay_signal(signal),
            Event::UserEvent(UserEvent::ConfigFileChanged) => self.reload_config_from_disk(),
            Event::LoopDestroyed => self.state = AppState::Exiting,
            _ => {}
        }

        if matches!(self.state, AppState::Exiting) {
            *control_flow = ControlFlow::Exit;
        }
    }

    fn next_control_flow(&self) -> ControlFlow {
        if self.state == AppState::Exiting {
            ControlFlow::Exit
        } else {
            ControlFlow::Wait
        }
    }

    fn initialize(&mut self) {
        match TrayHandles::new() {
            Ok(tray) => self.tray = Some(tray),
            Err(error) => error!(?error, "failed to create tray"),
        }

        match RegisteredHotkey::register(&self.config.general.hotkey) {
            Ok(hotkey) => {
                info!(hotkey = ?hotkey.hotkey(), "registered global hotkey");
                self.hotkey = Some(hotkey);
            }
            Err(error) => error!(
                ?error,
                hotkey = self.config.general.hotkey,
                "failed to register hotkey"
            ),
        }

        self.spawn_config_watcher();
        self.prewarm_overlay();
    }

    fn spawn_config_watcher(&self) {
        let proxy = self.event_proxy.clone();
        let config_path = self.paths.config_file.clone();
        thread::spawn(move || {
            let mut last_stamp = config_file_stamp(&config_path);
            loop {
                thread::sleep(Duration::from_millis(600));
                let next_stamp = config_file_stamp(&config_path);
                if next_stamp != last_stamp {
                    last_stamp = next_stamp;
                    let _ = proxy.send_event(UserEvent::ConfigFileChanged);
                }
            }
        });
    }

    fn reload_config_from_disk(&mut self) {
        let next_stamp = config_file_stamp(&self.paths.config_file);
        if self.config_file_stamp == next_stamp {
            return;
        }
        self.config_file_stamp = next_stamp;

        let previous = self.config.clone();
        let next = match crate::config::load_from_path(&self.paths.config_file) {
            Ok(config) => config,
            Err(error) => {
                warn!(?error, path = ?self.paths.config_file, "failed to reload config from disk");
                return;
            }
        };

        if let Err(error) = self.apply_runtime_config(next.clone()) {
            warn!(
                ?error,
                "failed to apply reloaded config; restoring previous config"
            );
            if let Err(write_error) =
                crate::config::write_config(&self.paths.config_file, &previous)
            {
                warn!(
                    ?write_error,
                    "failed to restore previous config after reload error"
                );
            }
            self.config_file_stamp = config_file_stamp(&self.paths.config_file);
            return;
        }

        info!("configuration reloaded");
    }

    fn apply_runtime_config(&mut self, next: AppConfig) -> anyhow::Result<()> {
        fs::create_dir_all(&next.general.save_dir)?;
        if self.config.general.hotkey != next.general.hotkey {
            let replacement = RegisteredHotkey::register(&next.general.hotkey)?;
            self.hotkey = Some(replacement);
            info!(hotkey = next.general.hotkey, "re-registered global hotkey");
        }
        self.config = next;
        Ok(())
    }

    fn open_settings_window(&self) {
        if let Err(error) = settings::open_or_focus() {
            warn!(?error, "failed to open settings window");
        }
    }

    fn prewarm_overlay(&mut self) {
        if self.overlay.is_some() {
            return;
        }

        let target = match capture::current_monitor_target() {
            Ok(target) => target,
            Err(error) => {
                warn!(?error, "failed to prepare hidden overlay cache");
                return;
            }
        };

        let proxy = self.event_proxy.clone();
        match OverlaySession::new(target, move |signal| {
            let _ = proxy.send_event(UserEvent::Overlay(signal));
        }) {
            Ok(overlay) => {
                self.overlay = Some(overlay);
                info!("overlay prewarmed");
            }
            Err(error) => warn!(?error, "failed to create overlay window"),
        }
    }

    fn handle_hotkey_event(&mut self, event: GlobalHotKeyEvent) {
        if self.state != AppState::Idle {
            return;
        }

        if self
            .hotkey
            .as_ref()
            .is_some_and(|hotkey| hotkey.matches_event(&event))
        {
            self.start_selection();
        }
    }

    fn handle_tray_menu_event(&mut self, control_flow: &mut ControlFlow, event: MenuEvent) {
        let action = self
            .tray
            .as_ref()
            .and_then(|tray| tray.resolve_action(&event.id));
        let Some(action) = action else {
            return;
        };

        match action {
            TrayAction::Capture => self.start_selection(),
            TrayAction::CaptureWindow => self.capture_ui_element(capture::UiCaptureKind::Window),
            TrayAction::CaptureControl => self.capture_ui_element(capture::UiCaptureKind::Control),
            TrayAction::OpenSettings => self.open_settings_window(),
            TrayAction::OpenSaveDir => {
                if let Err(error) = open_directory(&self.config.general.save_dir) {
                    warn!(?error, "failed to open save directory");
                }
            }
            TrayAction::OpenConfigDir => {
                if let Err(error) = open_directory(&self.paths.config_dir) {
                    warn!(?error, "failed to open config directory");
                }
            }
            TrayAction::Exit => {
                self.hide_overlay();
                self.close_pin_windows();
                self.state = AppState::Exiting;
                *control_flow = ControlFlow::Exit;
            }
        }
    }

    fn start_selection(&mut self) {
        if self.state != AppState::Idle {
            return;
        }

        self.state = AppState::Capturing;
        let (cursor_x, cursor_y) = match capture::current_cursor_position() {
            Ok(position) => position,
            Err(error) => {
                error!(?error, "failed to read cursor position");
                self.state = AppState::Idle;
                return;
            }
        };

        let target = match capture::target_for_point(cursor_x, cursor_y) {
            Ok(target) => target,
            Err(error) => {
                error!(?error, "failed to prepare capture target");
                self.state = AppState::Idle;
                return;
            }
        };

        if self.overlay.is_none() {
            self.prewarm_overlay();
        }

        let Some(overlay) = self.overlay.as_mut() else {
            self.state = AppState::Idle;
            return;
        };

        match overlay.show(
            target,
            cursor_x,
            cursor_y,
            &self.config.annotation_defaults,
            &self.config.ocr,
            &self.config.translation,
        ) {
            Ok(()) => {
                self.state = AppState::Selecting;
                info!(startup = ?self.startup_mode, "overlay opened");
            }
            Err(error) => {
                error!(?error, "failed to activate overlay window");
                self.state = AppState::Idle;
            }
        }
    }

    fn capture_ui_element(&mut self, kind: capture::UiCaptureKind) {
        if self.state != AppState::Idle {
            return;
        }

        self.state = AppState::Capturing;
        match capture::capture_ui_element_under_cursor(kind) {
            Ok(image) => {
                info!(?kind, "captured ui element under cursor");
                self.finish_capture(image);
            }
            Err(error) => {
                error!(?error, ?kind, "failed to capture ui element under cursor");
                self.state = AppState::Idle;
            }
        }
    }

    fn handle_overlay_signal(&mut self, signal: OverlaySignal) {
        match signal {
            OverlaySignal::Cancelled => {
                info!(startup = ?self.startup_mode, "selection cancelled");
                self.state = AppState::Idle;
            }
            OverlaySignal::Completed(image) => {
                self.finish_capture(image);
                self.state = AppState::Idle;
            }
            OverlaySignal::Pinned(capture) => {
                self.show_pin_window(capture);
                self.state = AppState::Idle;
            }
        }
    }

    fn hide_overlay(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.hide();
        }
    }

    fn show_pin_window(&mut self, capture: PinnedCapture) {
        self.pin_windows.retain(|window| window.is_alive());
        match PinWindow::show(
            capture.image,
            capture.screen_x,
            capture.screen_y,
            self.config.general.save_dir.clone(),
            &self.config.pin_defaults,
        ) {
            Ok(window) => {
                self.pin_windows.push(window);
                info!(
                    count = self.pin_windows.len(),
                    x = capture.screen_x,
                    y = capture.screen_y,
                    "pin window opened"
                );
            }
            Err(error) => {
                error!(?error, "failed to open pin window");
            }
        }
    }

    fn close_pin_windows(&mut self) {
        for window in &self.pin_windows {
            window.close();
        }
        self.pin_windows.clear();
    }

    fn finish_capture(&mut self, image: image::RgbaImage) {
        self.state = AppState::Capturing;
        match output::process_capture(image, &self.config) {
            Ok(result) => {
                info!(path = ?result.saved_path, image_width = result.image.width(), image_height = result.image.height(), "capture completed");
            }
            Err(error) => {
                error!(?error, "failed to process capture output");
            }
        }
        self.state = AppState::Idle;
    }
}

fn open_directory(path: &Path) -> anyhow::Result<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}

fn config_file_stamp(path: &Path) -> Option<ConfigFileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(ConfigFileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}
