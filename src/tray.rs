use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuId, MenuItem},
};

pub struct TrayHandles {
    _tray_icon: TrayIcon,
    capture_id: MenuId,
    capture_window_id: MenuId,
    capture_control_id: MenuId,
    settings_id: MenuId,
    open_save_dir_id: MenuId,
    open_config_dir_id: MenuId,
    exit_id: MenuId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Capture,
    CaptureWindow,
    CaptureControl,
    OpenSettings,
    OpenSaveDir,
    OpenConfigDir,
    Exit,
}

impl TrayHandles {
    pub fn new() -> Result<Self> {
        let menu = Menu::new();
        let capture = MenuItem::new("截图", true, None);
        let capture_window = MenuItem::new("窗口截图", true, None);
        let capture_control = MenuItem::new("控件截图", true, None);
        let settings = MenuItem::new("设置", true, None);
        let open_save_dir = MenuItem::new("打开截图目录", true, None);
        let open_config_dir = MenuItem::new("打开配置目录", true, None);
        let exit = MenuItem::new("退出", true, None);

        menu.append(&capture)
            .context("failed to add capture menu item")?;
        menu.append(&capture_window)
            .context("failed to add window capture menu item")?;
        menu.append(&capture_control)
            .context("failed to add control capture menu item")?;
        menu.append(&settings)
            .context("failed to add settings menu item")?;
        menu.append(&open_save_dir)
            .context("failed to add open save directory menu item")?;
        menu.append(&open_config_dir)
            .context("failed to add open config directory menu item")?;
        menu.append(&exit).context("failed to add exit menu item")?;

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("OpenCapt")
            .with_icon(build_icon()?)
            .with_menu(Box::new(menu))
            .build()
            .context("failed to build tray icon")?;

        Ok(Self {
            _tray_icon: tray_icon,
            capture_id: capture.id().clone(),
            capture_window_id: capture_window.id().clone(),
            capture_control_id: capture_control.id().clone(),
            settings_id: settings.id().clone(),
            open_save_dir_id: open_save_dir.id().clone(),
            open_config_dir_id: open_config_dir.id().clone(),
            exit_id: exit.id().clone(),
        })
    }

    pub fn resolve_action(&self, menu_id: &MenuId) -> Option<TrayAction> {
        if *menu_id == self.capture_id {
            return Some(TrayAction::Capture);
        }
        if *menu_id == self.capture_window_id {
            return Some(TrayAction::CaptureWindow);
        }
        if *menu_id == self.capture_control_id {
            return Some(TrayAction::CaptureControl);
        }
        if *menu_id == self.settings_id {
            return Some(TrayAction::OpenSettings);
        }
        if *menu_id == self.open_save_dir_id {
            return Some(TrayAction::OpenSaveDir);
        }
        if *menu_id == self.open_config_dir_id {
            return Some(TrayAction::OpenConfigDir);
        }
        if *menu_id == self.exit_id {
            return Some(TrayAction::Exit);
        }
        None
    }
}

fn build_icon() -> Result<Icon> {
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity(width * height * 4);

    for y in 0..height {
        for x in 0..width {
            let (r, g, b, a) = if x > 4 && x < 28 && y > 6 && y < 24 {
                if x > 8 && x < 24 && y > 10 && y < 20 {
                    (243, 247, 250, 255)
                } else {
                    (52, 131, 235, 255)
                }
            } else {
                (0, 0, 0, 0)
            };

            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).context("failed to construct tray icon")
}
