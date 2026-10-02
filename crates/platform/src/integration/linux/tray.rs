//! The tray icon: a StatusNotifierItem over D-Bus, through `ksni`. KDE,
//! Ubuntu's GNOME (its AppIndicator extension), Cinnamon, XFCE, LXQt and
//! most bars show it; stock GNOME without the extension doesn't, and there
//! opening the app again still shows the window.
//!
//! A click opens the main window; the menu shows the active game and common
//! actions, with Quit at the bottom.
//! The icon is the app icon's images from the embedded .ico, so it needs no
//! icon theme (an AppImage installs none).

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip};

use super::{Signal, emit};
use crate::integration::{MenuSource, TrayDialog, TrayGameAction, TrayIcon, icon_png};

const ICO: &[u8] = include_bytes!("../../../../../assets/icon.ico");

pub struct Tray(Handle<Item>);

impl Tray {
    /// Shows the icon. The desktop's tray (the StatusNotifierWatcher) may
    /// come up after the host at sign-in; the icon appears once it does.
    pub fn start(menu_source: MenuSource) -> Result<Tray, String> {
        Item { icons: ico_icons(ICO), menu_source }
            .assume_sni_available(true)
            .spawn()
            .map(Tray)
            .map_err(|e| format!("the tray is unavailable: {e}"))
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.0.shutdown().wait();
    }
}

pub struct Item {
    icons: Vec<Icon>,
    menu_source: MenuSource,
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "savescummer".into()
    }

    fn title(&self) -> String {
        "SaveScummer".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "SaveScummer".into(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        emit(Signal::OpenMainWindow);
    }

    fn menu_about_to_show(&mut self) {
        // ksni refreshes the menu after this callback, so game availability
        // reflects the latest host state each time the user opens it.
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let snapshot = (self.menu_source)();
        let game_action = |label: &str, action: TrayGameAction, icon: TrayIcon, enabled: bool| {
            let game = snapshot.game.clone();
            StandardItem {
                label: label.into(),
                enabled,
                icon_data: icon_png(icon).to_vec(),
                activate: Box::new(move |_: &mut Self| {
                    if let Some(game) = &game {
                        emit(Signal::TrayGame { game: game.clone(), action });
                    }
                }),
                ..Default::default()
            }
            .into()
        };
        vec![
            StandardItem {
                label: snapshot.name.unwrap_or_else(|| "No active game".into()),
                enabled: false,
                ..Default::default()
            }
            .into(),
            if snapshot.running {
                game_action("Stop", TrayGameAction::Stop, TrayIcon::Stop, snapshot.stop)
            } else {
                game_action("Play", TrayGameAction::Play, TrayIcon::Play, snapshot.play)
            },
            game_action("Save checkpoint", TrayGameAction::Save, TrayIcon::Save, snapshot.save),
            game_action("Load latest checkpoint", TrayGameAction::Load, TrayIcon::Load, snapshot.load),
            MenuItem::Separator,
            StandardItem {
                label: "Main window".into(),
                icon_data: icon_png(TrayIcon::Main).to_vec(),
                activate: Box::new(|_: &mut Self| emit(Signal::OpenMainWindow)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Add custom game…".into(),
                icon_data: icon_png(TrayIcon::Add).to_vec(),
                activate: Box::new(|_: &mut Self| emit(Signal::OpenDialog(TrayDialog::Add))),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Scan for games".into(),
                icon_data: icon_png(TrayIcon::Scan).to_vec(),
                activate: Box::new(|_: &mut Self| emit(Signal::Scan)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings…".into(),
                icon_data: icon_png(TrayIcon::Settings).to_vec(),
                activate: Box::new(|_: &mut Self| emit(Signal::OpenDialog(TrayDialog::Settings))),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "About SaveScummer…".into(),
                icon_data: icon_png(TrayIcon::About).to_vec(),
                activate: Box::new(|_: &mut Self| emit(Signal::OpenDialog(TrayDialog::About))),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_: &mut Self| emit(Signal::Exit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Every 32-bit BMP image in an .ico, as SNI pixmaps (ARGB, big-endian).
/// PNG images (the large sizes) are skipped: the tray never draws them.
fn ico_icons(ico: &[u8]) -> Vec<Icon> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(ico.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(ico.get(i..i + 4)?.try_into().ok()?));
    let Some(count) = u16_at(4).filter(|_| u16_at(2) == Some(1)) else { return Vec::new() };
    (0..count as usize)
        .filter_map(|i| {
            let at = 6 + 16 * i;
            let offset = u32_at(at + 12)? as usize;
            bmp_icon(ico.get(offset..offset.checked_add(u32_at(at + 8)? as usize)?)?)
        })
        .collect()
}

/// One .ico BMP image: a BITMAPINFOHEADER whose height counts the AND mask
/// too, then bottom-up BGRA rows.
fn bmp_icon(image: &[u8]) -> Option<Icon> {
    let i32_at = |i: usize| Some(i32::from_le_bytes(image.get(i..i + 4)?.try_into().ok()?));
    let header = u32::from_le_bytes(image.get(0..4)?.try_into().ok()?) as usize;
    let bits = u16::from_le_bytes(image.get(14..16)?.try_into().ok()?);
    let (width, height) = (i32_at(4)?, i32_at(8)? / 2);
    if header < 40 || bits != 32 || width <= 0 || height <= 0 {
        return None;
    }
    let row = width as usize * 4;
    let pixels = image.get(header..header + row * height as usize)?;
    let mut data = Vec::with_capacity(pixels.len());
    for line in pixels.chunks_exact(row).rev() {
        for [b, g, r, a] in line.as_chunks::<4>().0 {
            data.extend_from_slice(&[*a, *r, *g, *b]);
        }
    }
    Some(Icon { width, height, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_icon_has_small_images() {
        let icons = ico_icons(ICO);
        assert!(icons.iter().any(|icon| icon.width == 16 && icon.height == 16));
        assert!(icons.iter().any(|icon| icon.width == 32));
        for icon in &icons {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        }
        // Not fully transparent: the alpha channel came through.
        assert!(icons[0].data.as_chunks::<4>().0.iter().any(|argb| argb[0] == 0xff));
        assert!(ico_icons(b"not an icon").is_empty());
    }
}
