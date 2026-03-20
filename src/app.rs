use crate::{
    StartupMode, annotate, capture,
    config::{AppConfig, AppPaths},
    hotkey::RegisteredHotkey,
    output,
    overlay::{OverlaySession, OverlaySignal},
    tray::{TrayAction, TrayHandles},
};
use global_hotkey::GlobalHotKeyEvent;
use std::{process::Command, thread};
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
    Annotating,
    Exiting,
}

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    HotKey(GlobalHotKeyEvent),
    TrayMenu(MenuEvent),
    Overlay(OverlaySignal),
    Annotation(annotate::EditorOutcome),
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
    paths: AppPaths,
    state: AppState,
    startup_mode: StartupMode,
    event_proxy: EventLoopProxy<UserEvent>,
    tray: Option<TrayHandles>,
    hotkey: Option<RegisteredHotkey>,
    overlay: Option<OverlaySession>,
    pending_target: Option<capture::CaptureTarget>,
    pending_annotation_fallback: Option<image::RgbaImage>,
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
            paths,
            state: AppState::Idle,
            startup_mode: startup_mode.clone(),
            event_proxy,
            tray: None,
            hotkey: None,
            overlay: None,
            pending_target: None,
            pending_annotation_fallback: None,
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
            Event::UserEvent(UserEvent::Annotation(result)) => {
                self.handle_annotation_result(result)
            }
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

        match RegisteredHotkey::register(&self.config.hotkey) {
            Ok(hotkey) => {
                info!(hotkey = ?hotkey.hotkey(), "registered global hotkey");
                self.hotkey = Some(hotkey);
            }
            Err(error) => error!(
                ?error,
                hotkey = self.config.hotkey,
                "failed to register hotkey"
            ),
        }

        self.prewarm_overlay();
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
            TrayAction::OpenSaveDir => {
                if let Err(error) = open_directory(&self.config.save_dir) {
                    warn!(?error, "failed to open save directory");
                }
            }
            TrayAction::OpenConfigDir => {
                if let Err(error) = open_directory(&self.paths.config_dir) {
                    warn!(?error, "failed to open config directory");
                }
            }
            TrayAction::Exit => {
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

        self.pending_target = Some(target.clone());
        if self.overlay.is_none() {
            self.prewarm_overlay();
        }

        let Some(overlay) = self.overlay.as_mut() else {
            self.pending_target = None;
            self.state = AppState::Idle;
            return;
        };

        match overlay.show(target, cursor_x, cursor_y) {
            Ok(()) => {
                self.state = AppState::Selecting;
                info!(startup = ?self.startup_mode, "overlay opened");
            }
            Err(error) => {
                error!(?error, "failed to activate overlay window");
                self.pending_target = None;
                self.state = AppState::Idle;
            }
        }
    }

    fn handle_overlay_signal(&mut self, signal: OverlaySignal) {
        match signal {
            OverlaySignal::Cancelled => {
                info!(startup = ?self.startup_mode, "selection cancelled");
                self.pending_target = None;
                self.state = AppState::Idle;
            }
            OverlaySignal::Confirmed(rect) => {
                self.state = AppState::Capturing;
                let Some(target) = self.pending_target.take() else {
                    self.state = AppState::Idle;
                    return;
                };

                match capture::capture_region(&target, rect) {
                    Ok(fallback_image) => {
                        let placement = annotate::EditorPlacement {
                            screen_x: target.origin_x + rect.x,
                            screen_y: target.origin_y + rect.y,
                            monitor_x: target.origin_x,
                            monitor_y: target.origin_y,
                            monitor_width: target.width,
                            monitor_height: target.height,
                            selection_width: rect.width,
                            selection_height: rect.height,
                            scale_milli: (target.scale_factor * 1000.0).round() as u32,
                        };
                        if let Err(error) = self.launch_annotation(
                            target.background.clone(),
                            placement,
                            fallback_image.clone(),
                        ) {
                            error!(
                                ?error,
                                "failed to launch annotation editor; falling back to direct output"
                            );
                            self.finish_capture(fallback_image);
                        }
                    }
                    Err(error) => {
                        error!(?error, "failed to capture selected region");
                        self.state = AppState::Idle;
                    }
                }
            }
        }
    }

    fn launch_annotation(
        &mut self,
        monitor_image: image::RgbaImage,
        placement: annotate::EditorPlacement,
        fallback_image: image::RgbaImage,
    ) -> anyhow::Result<()> {
        let launch = annotate::spawn_editor(&monitor_image, placement)?;
        self.pending_annotation_fallback = Some(fallback_image);
        let proxy = self.event_proxy.clone();
        thread::spawn(move || {
            let result = annotate::wait_for_editor(launch);
            let _ = proxy.send_event(UserEvent::Annotation(result));
        });
        self.state = AppState::Annotating;
        info!("annotation editor launched");
        Ok(())
    }

    fn handle_annotation_result(&mut self, result: annotate::EditorOutcome) {
        match result {
            annotate::EditorOutcome::Confirmed {
                output_path,
                temp_dir,
            } => {
                self.pending_annotation_fallback = None;
                match annotate::load_output_image(&output_path) {
                    Ok(image) => self.finish_capture(image),
                    Err(error) => {
                        error!(?error, output = ?output_path, "failed to load annotated output")
                    }
                }
                annotate::cleanup_temp_dir(&temp_dir);
                self.state = AppState::Idle;
            }
            annotate::EditorOutcome::Cancelled { temp_dir } => {
                self.pending_annotation_fallback = None;
                info!("annotation cancelled");
                annotate::cleanup_temp_dir(&temp_dir);
                self.state = AppState::Idle;
            }
            annotate::EditorOutcome::Failed { message, temp_dir } => {
                error!(message = %message, "annotation editor failed");
                if let Some(image) = self.pending_annotation_fallback.take() {
                    self.finish_capture(image);
                }
                if let Some(temp_dir) = temp_dir {
                    annotate::cleanup_temp_dir(&temp_dir);
                }
                self.state = AppState::Idle;
            }
        }
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

fn open_directory(path: &std::path::Path) -> anyhow::Result<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}
