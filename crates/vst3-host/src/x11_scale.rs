//! The scale plug-in editors are drawn at on Linux, passed to views that implement
//! `IPlugViewContentScaleSupport`.
//!
//! X11 (and XWayland) has one scale for the whole display, so this is read once per editor from
//! the same places GTK reads Marbble's own scale: `GDK_SCALE`, then the XSETTINGS
//! `Gdk/WindowScalingFactor`. Desktops that set neither but set `Xft.dpi` (a common setup on
//! Hyprland, sway and i3 with `xwayland force_zero_scaling`) get `Xft.dpi / 96`, rounded to a
//! quarter. Everything else is 1.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

/// Editors are never scaled beyond this, whatever a setting says.
const MAX_SCALE: f64 = 4.0;

/// What the display says about scaling. Each field is `None` when its source doesn't say.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ScaleSources {
    /// The `GDK_SCALE` environment variable.
    pub gdk_scale: Option<String>,
    /// XSETTINGS `Gdk/WindowScalingFactor`.
    pub window_scale: Option<i32>,
    /// XSETTINGS `Xft/DPI`, in 1/1024 dots per inch.
    pub xsettings_dpi: Option<i32>,
    /// The `RESOURCE_MANAGER` property on the root window (what `xrdb` sets).
    pub resources: Option<String>,
}

/// The editor scale, and which source gave it (for the log).
pub(crate) fn choose(s: &ScaleSources) -> (f64, &'static str) {
    // GTK takes GDK_SCALE only as a whole number of at least 1.
    if let Some(n) = s.gdk_scale.as_deref().and_then(|v| v.trim().parse::<i32>().ok()).filter(|n| *n >= 1) {
        return (clamp(n as f64), "GDK_SCALE");
    }
    if let Some(n) = s.window_scale.filter(|n| *n >= 1) {
        return (clamp(n as f64), "Gdk/WindowScalingFactor");
    }
    let dpi = s.resources.as_deref().and_then(xft_dpi).or(s.xsettings_dpi.filter(|d| *d > 0).map(|d| d as f64 / 1024.0));
    if let Some(dpi) = dpi {
        return (clamp((dpi / 96.0 * 4.0).round() / 4.0), "Xft.dpi");
    }
    (1.0, "default")
}

fn clamp(s: f64) -> f64 {
    if s.is_finite() {
        s.clamp(1.0, MAX_SCALE)
    } else {
        1.0
    }
}

/// `Xft.dpi` from an X resource database string (`Xft.dpi:\t192`), by its name or its class
/// (`Xft.Dpi`), as plug-ins look it up. Wildcard entries (`*dpi`) aren't matched.
pub(crate) fn xft_dpi(resources: &str) -> Option<f64> {
    resources.lines().rev().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        let key = key.trim();
        (key == "Xft.dpi" || key == "Xft.Dpi").then(|| value.trim().parse::<f64>().ok()).flatten().filter(|d| *d > 0.0)
    })
}

/// The integer settings `Gdk/WindowScalingFactor` and `Xft/DPI` from an `_XSETTINGS_SETTINGS`
/// property (the XSETTINGS specification's wire format). A malformed blob gives what was read
/// before the damage.
pub(crate) fn parse_xsettings(data: &[u8]) -> (Option<i32>, Option<i32>) {
    let (mut window_scale, mut dpi) = (None, None);
    let Some(&order) = data.first() else { return (None, None) };
    let big = order == 1; // MSBFirst
    let u16_at = |i: usize| data.get(i..i + 2).map(|b| if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let u32_at = |i: usize| {
        data.get(i..i + 4).map(|b| {
            let b = [b[0], b[1], b[2], b[3]];
            if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            }
        })
    };
    let pad = |n: usize| (n + 3) & !3;
    let Some(count) = u32_at(8) else { return (None, None) };
    let mut at = 12;
    for _ in 0..count {
        let (Some(&kind), Some(name_len)) = (data.get(at), u16_at(at + 2)) else { break };
        let name_at = at + 4;
        let Some(name) = data.get(name_at..name_at + name_len as usize) else { break };
        let value_at = name_at + pad(name_len as usize) + 4; // + last-change serial
        at = match kind {
            0 => {
                let Some(v) = u32_at(value_at) else { break };
                match name {
                    b"Gdk/WindowScalingFactor" => window_scale = Some(v as i32),
                    b"Xft/DPI" => dpi = Some(v as i32),
                    _ => {}
                }
                value_at + 4
            }
            1 => {
                let Some(len) = u32_at(value_at) else { break };
                value_at + 4 + pad(len as usize)
            }
            2 => value_at + 8,
            _ => break,
        };
    }
    (window_scale, dpi)
}

/// Reads the sources from the X server `conn` is connected to.
pub(crate) fn read(conn: &RustConnection, screen_num: usize) -> ScaleSources {
    let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let property =
        |window: u32, prop: u32| conn.get_property(false, window, prop, AtomEnum::ANY, 0, 1 << 16).ok().and_then(|c| c.reply().ok()).map(|r| r.value);
    let root = conn.setup().roots[screen_num].root;
    let resources = property(root, AtomEnum::RESOURCE_MANAGER.into()).map(|v| String::from_utf8_lossy(&v).into_owned());
    let (window_scale, xsettings_dpi) = (|| {
        let selection = atom(&format!("_XSETTINGS_S{screen_num}"))?;
        let owner = conn.get_selection_owner(selection).ok()?.reply().ok()?.owner;
        if owner == 0 {
            return None;
        }
        Some(parse_xsettings(&property(owner, atom("_XSETTINGS_SETTINGS")?)?))
    })()
    .unwrap_or((None, None));
    ScaleSources { gdk_scale: std::env::var("GDK_SCALE").ok(), window_scale, xsettings_dpi, resources }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `_XSETTINGS_SETTINGS` blob: a string, a colour and the integers given.
    fn blob(big: bool, ints: &[(&str, i32)]) -> Vec<u8> {
        let u16b = |v: u16| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let u32b = |v: u32| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let mut d = vec![big as u8, 0, 0, 0];
        d.extend(u32b(7)); // serial
        d.extend(u32b(2 + ints.len() as u32));
        let head = |d: &mut Vec<u8>, kind: u8, name: &str| {
            d.extend([kind, 0]);
            d.extend(u16b(name.len() as u16));
            d.extend(name.as_bytes());
            d.resize((d.len() + 3) & !3, 0);
            d.extend(u32b(0)); // last-change serial
        };
        head(&mut d, 1, "Net/ThemeName");
        d.extend(u32b(5));
        d.extend(b"Adw\0d\0\0\0");
        head(&mut d, 2, "Gtk/Color");
        d.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        for (name, v) in ints {
            head(&mut d, 0, name);
            d.extend(u32b(*v as u32));
        }
        d
    }

    #[test]
    fn xsettings_in_both_byte_orders() {
        for big in [false, true] {
            let d = blob(big, &[("Gtk/CursorBlinkTime", 1200), ("Gdk/WindowScalingFactor", 2), ("Xft/DPI", 192 * 1024)]);
            assert_eq!(parse_xsettings(&d), (Some(2), Some(192 * 1024)), "big endian: {big}");
        }
        assert_eq!(parse_xsettings(&blob(false, &[("Xft/Antialias", 1)])), (None, None));
    }

    #[test]
    fn malformed_xsettings_never_panic() {
        let d = blob(false, &[("Gdk/WindowScalingFactor", 3), ("Xft/DPI", 144 * 1024)]);
        for n in 0..d.len() {
            parse_xsettings(&d[..n]);
        }
        // Cut inside the last setting: the first is still read.
        assert_eq!(parse_xsettings(&d[..d.len() - 2]), (Some(3), None));
        assert_eq!(parse_xsettings(&[]), (None, None));
        let mut huge = d.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_xsettings(&huge), (Some(3), Some(144 * 1024)));
    }

    #[test]
    fn xft_dpi_from_resources() {
        assert_eq!(xft_dpi("Xcursor.size:\t24\nXft.dpi:\t192\n"), Some(192.0));
        assert_eq!(xft_dpi("Xft.Dpi: 144"), Some(144.0));
        assert_eq!(xft_dpi("Xft.dpi: 96\nXft.dpi: 120"), Some(120.0));
        assert_eq!(xft_dpi("Xft.dpi: 0\n"), None);
        assert_eq!(xft_dpi("xft.dpi: 192\nXft.dpiX: 192\nXft.antialias: 1"), None);
        assert_eq!(xft_dpi(""), None);
    }

    #[test]
    fn choice_follows_gtk_then_xft_dpi() {
        let s = |gdk: Option<&str>, ws: Option<i32>, xs_dpi: Option<i32>, res: Option<&str>| ScaleSources {
            gdk_scale: gdk.map(String::from),
            window_scale: ws,
            xsettings_dpi: xs_dpi,
            resources: res.map(String::from),
        };
        assert_eq!(choose(&ScaleSources::default()), (1.0, "default"));
        assert_eq!(choose(&s(Some("2"), Some(1), None, Some("Xft.dpi: 96"))), (2.0, "GDK_SCALE"));
        // GTK ignores a GDK_SCALE that isn't a whole number of at least 1.
        assert_eq!(choose(&s(Some("1.5"), Some(2), None, None)), (2.0, "Gdk/WindowScalingFactor"));
        assert_eq!(choose(&s(Some("0"), None, None, None)), (1.0, "default"));
        // GNOME at 100% with large text: the window scale says 1, the larger DPI is for text.
        assert_eq!(choose(&s(None, Some(1), Some(120 * 1024), Some("Xft.dpi: 120"))), (1.0, "Gdk/WindowScalingFactor"));
        assert_eq!(choose(&s(None, None, None, Some("Xft.dpi: 144"))), (1.5, "Xft.dpi"));
        assert_eq!(choose(&s(None, None, Some(192 * 1024), None)), (2.0, "Xft.dpi"));
        assert_eq!(choose(&s(None, None, Some(192 * 1024), Some("Xft.dpi: 120"))), (1.25, "Xft.dpi"));
        // Near 96 is 1; rounding is to a quarter; out-of-range values are held to 1..4.
        assert_eq!(choose(&s(None, None, None, Some("Xft.dpi: 100"))), (1.0, "Xft.dpi"));
        assert_eq!(choose(&s(None, None, None, Some("Xft.dpi: 170"))), (1.75, "Xft.dpi"));
        assert_eq!(choose(&s(None, None, None, Some("Xft.dpi: 48"))), (1.0, "Xft.dpi"));
        assert_eq!(choose(&s(Some("9"), None, None, None)), (4.0, "GDK_SCALE"));
    }
}
