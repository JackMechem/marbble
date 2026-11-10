//! The menu bar's spec, and submenus greyed out while none of their items applies.

use super::*;

fn walk<'a>(specs: &'a [Spec], out: &mut Vec<(&'a str, bool, Option<&'a str>)>) {
    for s in specs {
        match s {
            Item(id, _, accel) => out.push((id, false, *accel)),
            Check(id, _, accel) => out.push((id, true, *accel)),
            Radio(id, _) => out.push((id, true, None)),
            Sub(_, children) => walk(children, out),
            _ => {}
        }
    }
}

#[test]
fn ids_are_unique() {
    let bar = spec();
    let mut all = Vec::new();
    walk(&bar, &mut all);
    assert!(all.len() > 120, "only {} items", all.len());
    let mut ids: Vec<_> = all.iter().map(|(id, ..)| *id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "two items share an id");
}

#[test]
fn the_bar_holds_menus_and_one_of_each_list() {
    let bar = spec();
    assert!(bar.iter().all(|s| matches!(s, Sub(..))));
    let Sub("File", file) = &bar[1] else { panic!("File is second") };
    assert_eq!(file.iter().filter(|s| matches!(s, Spec::Revert)).count(), 1);
    assert_eq!(file.iter().filter(|s| matches!(s, Spec::Alternatives)).count(), 1);
}

#[test]
fn window_and_help_come_last() {
    let bar = spec();
    let names: Vec<_> = bar.iter().map(|s| if let Sub(n, _) = s { *n } else { "" }).collect();
    assert_eq!(names, ["Marbble", "File", "Edit", "Track", "Record", "Navigate", "View", "Window", "Help"]);
    let Sub("Window", window) = &bar[7] else { panic!() };
    assert!(matches!(window.last(), Some(Spec::Windows)), "the list of windows ends Window");
    // Every Help item but Key Commands and Check for Updates opens a page, and every page has
    // its item.
    let Sub("Help", help) = &bar[8] else { panic!() };
    let ids = ids_in(help);
    for (id, url) in HELP_PAGES {
        assert!(ids.contains(&id), "{id}");
        assert!(url.starts_with("https://"), "{url}");
    }
    assert_eq!(ids.len(), HELP_PAGES.len() + 2);
}

#[test]
fn a_submenu_is_greyed_out_when_none_of_its_items_applies() {
    let bar = spec();
    let Sub("Edit", edit) = &bar[2] else { panic!("Edit is third") };
    let Some(Sub("Repeat", repeat)) = edit.iter().find(|s| matches!(s, Sub("Repeat", _))) else { panic!() };
    let ids = ids_in(repeat);
    assert_eq!(ids, ["repeat:1", "repeat:2", "repeat:3", "repeat:4", "repeat:8", "repeat:16"]);
    let mut enabled = HashMap::new();
    // Nothing sent yet: as built, enabled.
    assert!(any_enabled(&ids, &enabled));
    for id in &ids {
        enabled.insert(id.to_string(), false);
    }
    assert!(!any_enabled(&ids, &enabled));
    enabled.insert("repeat:8".into(), true);
    assert!(any_enabled(&ids, &enabled));
    // A submenu's own submenus count.
    let Some(Sub("Select", select)) = edit.iter().find(|s| matches!(s, Sub("Select", _))) else { panic!() };
    assert!(ids_in(select).contains(&"sel-invert"));
}

#[test]
fn quit_is_an_ordinary_item_with_ctrl_q() {
    let bar = spec();
    let mut all = Vec::new();
    walk(&bar, &mut all);
    assert!(all.contains(&("quit", false, Some("CmdOrCtrl+Q"))));
}
